//! Parsed-expression cache for arithmetic expansion.
//!
//! The one-pass evaluator ([`super::one_pass`]) re-lexes and re-parses an
//! expression on every evaluation, which was ~17% of a hot
//! `i=$((i + 1))` loop. This module parses an expression once into a
//! tree and keeps the tree keyed by its text, so later evaluations of the
//! same text only walk the tree.
//!
//! Only expressions that need no `$`/backquote expansion before parsing
//! are cached, by their source text (an expanded text such as the
//! `n + 17` of `$((n + $1))` is typically new on every evaluation).
//! Variable values evaluated as expressions (`x=1+2; $((x))`) always take
//! the one-pass path: they recurse through each other, and a tree walk
//! per level would need more native stack than that evaluator.
//!
//! Equivalence with the one-pass evaluator: the tree records exactly the
//! structure that evaluator parses (both scan with the same
//! [`super::token::Cursor`] and its operator matchers), and [`eval`]
//! applies the same [`super::ops`] semantics, visiting it in the same
//! order with the same nesting bookkeeping — skipped operands (the
//! untaken `?:` branch, the right side of a short-circuited `&&`/`||`)
//! are walked in no-evaluation mode just as the one-pass evaluator parses
//! them — so results, side effects and runtime errors (division by zero,
//! readonly assignment, nesting and recursion limits) come out the same.
//! A text with a syntax error never gets a tree: it always takes the
//! one-pass path, whose diagnostic may depend on side effects evaluated
//! before the error is reached.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::ops::{BinOp, CompoundOp, UnOp};
use super::token::{self, Cursor};
use super::var::{assign, var_operand};
use super::{MAX_NEST_DEPTH, arith_err};
use crate::env::ShellEnv;
use crate::exec::stack_guard::descent_stack_exhausted;

/// Texts longer than this are not cached (nor parsed into a tree): a
/// one-off huge expression gains nothing and would pin memory.
const MAX_CACHED_TEXT: usize = 256;

/// Number of distinct texts kept before the cache is emptied.
const MAX_ENTRIES: usize = 256;

/// Texts nested deeper than this get no tree. The tree walk's frames
/// differ in size from the one-pass evaluator's, so near the native
/// stack limit the two could disagree on whether an expression is
/// "nested too deeply"; deep texts always take the one-pass path, and
/// shallow ones stay far from any stack limit.
pub(super) const MAX_CACHED_NEST: u32 = 16;

thread_local! {
    /// Text → its tree, or `None` when it does not parse.
    static CACHE: RefCell<HashMap<Box<str>, Option<Rc<Node>>>> = RefCell::new(HashMap::new());
}

/// The cached tree for `text`, parsing and caching it on first sight.
/// `None` when `text` has a syntax error, is too long or nested too
/// deeply to cache; the caller then uses the one-pass evaluator.
pub(super) fn lookup(text: &str) -> Option<Rc<Node>> {
    if text.len() > MAX_CACHED_TEXT {
        return None;
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(text) {
            return entry.clone();
        }
        if cache.len() >= MAX_ENTRIES {
            cache.clear();
        }
        let tree = parse(text).map(Rc::new);
        cache.insert(text.into(), tree.clone());
        tree
    })
}

/// One parsed operator level. Left-associative chains are flat lists
/// (`a + b - c` is one `Bin`), as in the one-pass evaluator's loops, so
/// the tree is never deeper than the evaluator's recursion would be.
pub(super) enum Node {
    Num(i64),
    Var(Box<str>),
    /// `-x`, `+x`, `!x`, `~x`: each a nesting level.
    Unary(UnOp, Box<Node>),
    /// `( expr )`: a nesting level.
    Paren(Box<Node>),
    /// `name = rhs` (rhs a nesting level).
    Assign(Box<str>, Box<Node>),
    /// `name op= rhs` (rhs a nesting level).
    OpAssign(Box<str>, CompoundOp, Box<Node>),
    /// `a, b, …`
    Comma(Box<Node>, Vec<Node>),
    /// `cond ? then : else` (both branches nesting levels).
    Ternary(Box<[Node; 3]>),
    /// `a || b || …` (each right operand a nesting level).
    Or(Box<Node>, Vec<Node>),
    /// `a && b && …` (each right operand a nesting level).
    And(Box<Node>, Vec<Node>),
    /// One binary precedence level: `a op b op …`.
    Bin(Box<Node>, Vec<(BinOp, Node)>),
}

// ─── Parser ─────────────────────────────────────────────────────────────────
//
// A transcription of the one-pass evaluator's grammar that builds nodes
// instead of values; every failure is `None` (that evaluator reports it).

struct Parser<'a> {
    cur: Cursor<'a>,
    nest: u32,
}

// Parsing stops (no tree) as soon as `nest` would exceed
// `MAX_CACHED_NEST`; see `Parser::nested`.

fn parse(text: &str) -> Option<Node> {
    let mut p = Parser {
        cur: Cursor::new(text),
        nest: 0,
    };
    let root = p.comma()?;
    p.cur.skip_whitespace();
    p.cur.at_end().then_some(root)
}

impl Parser<'_> {
    fn nested(&mut self, parse: impl FnOnce(&mut Self) -> Option<Node>) -> Option<Node> {
        if self.nest >= MAX_NEST_DEPTH.min(MAX_CACHED_NEST) || descent_stack_exhausted() {
            return None;
        }
        self.nest += 1;
        let node = parse(self);
        self.nest -= 1;
        node
    }

    fn comma(&mut self) -> Option<Node> {
        let first = self.ternary()?;
        let mut rest = Vec::new();
        while self.cur.eat(b',') {
            rest.push(self.ternary()?);
        }
        Some(if rest.is_empty() {
            first
        } else {
            Node::Comma(Box::new(first), rest)
        })
    }

    fn ternary(&mut self) -> Option<Node> {
        let cond = self.logical_or()?;
        if !self.cur.eat(b'?') {
            return Some(cond);
        }
        let then = self.nested(Self::ternary)?;
        self.cur.eat(b':').then_some(())?;
        let other = self.nested(Self::ternary)?;
        Some(Node::Ternary(Box::new([cond, then, other])))
    }

    fn logical_or(&mut self) -> Option<Node> {
        let first = self.logical_and()?;
        let mut rest = Vec::new();
        while self.cur.eat_pair(*b"||") {
            rest.push(self.nested(Self::logical_and)?);
        }
        Some(if rest.is_empty() {
            first
        } else {
            Node::Or(Box::new(first), rest)
        })
    }

    fn logical_and(&mut self) -> Option<Node> {
        let first = self.bitwise_or()?;
        let mut rest = Vec::new();
        while self.cur.eat_pair(*b"&&") {
            rest.push(self.nested(Self::bitwise_or)?);
        }
        Some(if rest.is_empty() {
            first
        } else {
            Node::And(Box::new(first), rest)
        })
    }

    /// One left-associative binary level: operands from `operand`, the
    /// operator from the level's matcher `op` (see [`token`]).
    fn binary(
        &mut self,
        operand: fn(&mut Self) -> Option<Node>,
        op: fn(&Cursor) -> Option<(BinOp, usize)>,
    ) -> Option<Node> {
        let first = operand(self)?;
        let mut rest = Vec::new();
        while let Some(kind) = self.cur.binary_op(op) {
            rest.push((kind, operand(self)?));
        }
        Some(if rest.is_empty() {
            first
        } else {
            Node::Bin(Box::new(first), rest)
        })
    }

    fn bitwise_or(&mut self) -> Option<Node> {
        self.binary(Self::bitwise_xor, token::bit_or_op)
    }

    fn bitwise_xor(&mut self) -> Option<Node> {
        self.binary(Self::bitwise_and, token::bit_xor_op)
    }

    fn bitwise_and(&mut self) -> Option<Node> {
        self.binary(Self::equality, token::bit_and_op)
    }

    fn equality(&mut self) -> Option<Node> {
        self.binary(Self::relational, token::equality_op)
    }

    fn relational(&mut self) -> Option<Node> {
        self.binary(Self::shift, token::relational_op)
    }

    fn shift(&mut self) -> Option<Node> {
        self.binary(Self::additive, token::shift_op)
    }

    fn additive(&mut self) -> Option<Node> {
        self.binary(Self::multiplicative, token::additive_op)
    }

    fn multiplicative(&mut self) -> Option<Node> {
        self.binary(Self::unary, token::multiplicative_op)
    }

    fn unary(&mut self) -> Option<Node> {
        self.cur.skip_whitespace();
        self.cur.peek()?;
        let Some(op) = self.cur.unary_op() else {
            return self.primary();
        };
        let operand = self.nested(Self::unary)?;
        Some(Node::Unary(op, Box::new(operand)))
    }

    fn primary(&mut self) -> Option<Node> {
        self.cur.skip_whitespace();
        let ch = self.cur.peek()?;
        if ch == b'(' {
            self.cur.bump(1);
            let inner = self.nested(Self::comma)?;
            self.cur.eat(b')').then_some(())?;
            return Some(Node::Paren(Box::new(inner)));
        }
        if ch.is_ascii_digit() {
            return self.cur.number().ok().map(Node::Num);
        }
        if ch.is_ascii_alphabetic() || ch == b'_' {
            return self.name_or_assign();
        }
        None
    }

    fn name_or_assign(&mut self) -> Option<Node> {
        let name: Box<str> = self.cur.name().ok()?.into();
        self.cur.skip_whitespace();
        if let Some(op) = self.cur.compound_assign_op() {
            let rhs = self.nested(Self::ternary)?;
            return Some(Node::OpAssign(name, op, Box::new(rhs)));
        }
        if self.cur.peek() == Some(b'=') && self.cur.peek_at(1) != Some(b'=') {
            self.cur.bump(1);
            let rhs = self.nested(Self::ternary)?;
            return Some(Node::Assign(name, Box::new(rhs)));
        }
        Some(Node::Var(name))
    }
}

// ─── Evaluator ──────────────────────────────────────────────────────────────

/// Evaluate a cached tree at variable recursion depth `depth` and
/// syntactic nesting `nest` (see the module comment).
pub(super) fn eval(
    env: &mut ShellEnv,
    tree: &Node,
    depth: u32,
    nest: u32,
) -> crate::error::Result<i64> {
    Evaluator {
        env,
        depth,
        nest,
        noeval: false,
    }
    .eval(tree)
}

struct Evaluator<'a> {
    env: &'a mut ShellEnv,
    depth: u32,
    nest: u32,
    /// See `ArithParser::noeval`.
    noeval: bool,
}

impl Evaluator<'_> {
    /// Mirrors `ArithParser::nested`.
    fn nested(&mut self, skip: bool, node: &Node) -> crate::error::Result<i64> {
        if self.nest >= MAX_NEST_DEPTH || descent_stack_exhausted() {
            return Err(arith_err("expression nested too deeply"));
        }
        self.nest += 1;
        let saved = self.noeval;
        self.noeval |= skip;
        let result = self.eval(node);
        self.noeval = saved;
        self.nest -= 1;
        result
    }

    fn eval(&mut self, node: &Node) -> crate::error::Result<i64> {
        match node {
            Node::Num(v) => Ok(*v),
            Node::Var(name) => var_operand(self.env, name, self.depth, self.nest, self.noeval),
            Node::Unary(op, operand) => {
                let v = self.nested(false, operand)?;
                Ok(op.apply(v))
            }
            Node::Paren(inner) => self.nested(false, inner),
            Node::Assign(name, rhs) => {
                let val = self.nested(false, rhs)?;
                if !self.noeval {
                    assign(self.env, name, val)?;
                }
                Ok(val)
            }
            Node::OpAssign(name, op, rhs) => {
                let rhs = self.nested(false, rhs)?;
                if self.noeval {
                    return Ok(0);
                }
                let cur = var_operand(self.env, name, self.depth, self.nest, false)?;
                let val = op.apply(cur, rhs)?;
                assign(self.env, name, val)?;
                Ok(val)
            }
            Node::Comma(first, rest) => {
                let mut result = self.eval(first)?;
                for node in rest {
                    result = self.eval(node)?;
                }
                Ok(result)
            }
            Node::Ternary(parts) => {
                let [cond, then, other] = &**parts;
                let cond = self.eval(cond)?;
                let then = self.nested(cond == 0, then)?;
                let other = self.nested(cond != 0, other)?;
                Ok(if cond != 0 { then } else { other })
            }
            Node::Or(first, rest) => {
                let mut left = self.eval(first)?;
                for node in rest {
                    let right = self.nested(left != 0, node)?;
                    left = i64::from(left != 0 || right != 0);
                }
                Ok(left)
            }
            Node::And(first, rest) => {
                let mut left = self.eval(first)?;
                for node in rest {
                    let right = self.nested(left == 0, node)?;
                    left = i64::from(left != 0 && right != 0);
                }
                Ok(left)
            }
            Node::Bin(first, rest) => {
                let mut left = self.eval(first)?;
                for (op, node) in rest {
                    let right = self.eval(node)?;
                    left = op.apply(left, right, self.noeval)?;
                }
                Ok(left)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ArithParser;
    use super::*;

    /// Evaluate `text` with the one-pass evaluator only.
    fn one_pass(env: &mut ShellEnv, text: &str) -> crate::error::Result<i64> {
        ArithParser::new(env, text, 0, 0).eval_full()
    }

    fn fresh_env() -> ShellEnv {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("a", "7").unwrap();
        env.vars.set("b", "-3").unwrap();
        env.vars.set("e", "a*2+1").unwrap();
        env.vars.set("o", "010").unwrap();
        env.vars.set("z", "0").unwrap();
        env.vars.set("r", "1").unwrap();
        env.vars.set_readonly("r");
        env
    }

    fn var_snapshot(env: &ShellEnv) -> Vec<Option<String>> {
        ["a", "b", "c", "e", "z", "r"]
            .iter()
            .map(|n| env.vars.get(n).map(str::to_string))
            .collect()
    }

    /// Every expression must give the same value or the same error, and
    /// leave the same variables, through the tree as through the one-pass
    /// evaluator; and the tree path must actually be taken for those
    /// that parse.
    #[test]
    fn tree_evaluation_matches_one_pass_evaluator() {
        let cases = [
            "1+2*3",
            " a * 2 % 5 ",
            "a<<2>>1",
            "a < b, a > b, a <= 7, a >= 8, a == 7, a != 7",
            "a & 3 | 4 ^ 1",
            "!a + ~b - -a + +b",
            "(a + b) * (a - b)",
            "e + 1",
            "o + 0x10 + 010",
            "c = a * 2",
            "c += 5, c -= 1, c *= 2, c /= 3, c %= 4, c <<= 3, c >>= 1, c &= 7, c ^= 2, c |= 8",
            "z ? (c = 1) : (c = 2)",
            "a ? 1 : 1 / 0",
            "z && (c = 9)",
            "a || (c = 9)",
            "z && 1 / 0",
            "a || 1 % 0",
            "a && b || z",
            "1 / z",
            "1 % z",
            "c /= z",
            "c %= 0",
            "r = 5",
            "r += 1",
            "z && (r = 5)",
            "c = 3, 1 / z",
            "LINENO + 1",
            "x = y = 4",
            "a ? b ? 1 : 2 : 3",
            "-9223372036854775807 - 1",
            "(-9223372036854775807 - 1) / -1",
            "(-9223372036854775807 - 1) % -1",
            "1 << 70",
        ];
        for text in cases {
            let mut tree_env = fresh_env();
            let mut pass_env = fresh_env();
            let tree = lookup(text).unwrap_or_else(|| panic!("{text:?} has no tree"));
            let got = eval(&mut tree_env, &tree, 0, 0);
            let want = one_pass(&mut pass_env, text);
            assert_eq!(got, want, "{text:?}");
            assert_eq!(var_snapshot(&tree_env), var_snapshot(&pass_env), "{text:?}");
        }
    }

    #[test]
    fn syntax_errors_get_no_tree() {
        for text in [
            "1 +",
            "(1",
            "1 2",
            "09",
            "0x",
            "a ? 1",
            "é+1",
            "z && (1 +)",
            "1 |= 2",
            "",
        ] {
            assert!(parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn nesting_limit_matches_one_pass_at_a_nonzero_base() {
        // A variable's value is evaluated one nesting level deeper than
        // where it is referenced: the limit must fire at the same point.
        // Debug-build frames are large: run on a roomy thread (see
        // `arith::tests::test_deep_nesting_is_error_not_overflow`).
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let n = MAX_CACHED_NEST as usize;
                let text = format!("{}1{}", "(".repeat(n), ")".repeat(n));
                let tree = parse(&text).expect("within the cache limit");
                let mut env = fresh_env();
                assert_eq!(eval(&mut env, &tree, 0, 0), Ok(1));
                let edge = MAX_NEST_DEPTH - MAX_CACHED_NEST;
                for base in [edge, edge + 1] {
                    let got = eval(&mut env, &tree, 0, base);
                    let mut pass = ArithParser::new(&mut env, &text, 0, base);
                    assert_eq!(got, pass.eval_full(), "base {base}");
                }
                assert!(eval(&mut env, &tree, 0, edge + 1).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn deeply_nested_texts_get_no_tree() {
        // Deep texts take the one-pass path, whose stack use is the
        // reference for the "nested too deeply" limit.
        let n = MAX_CACHED_NEST as usize;
        assert!(parse(&format!("{}1{}", "(".repeat(n), ")".repeat(n))).is_some());
        assert!(parse(&format!("{}1{}", "(".repeat(n + 1), ")".repeat(n + 1))).is_none());
        assert!(parse(&format!("{}1", "!".repeat(n + 1))).is_none());
    }

    #[test]
    fn cache_is_bounded_and_keeps_failures() {
        for i in 0..(MAX_ENTRIES * 2) {
            assert!(lookup(&format!("{i} + 1")).is_some());
        }
        CACHE.with(|c| assert!(c.borrow().len() <= MAX_ENTRIES));
        assert!(lookup("1 +").is_none());
        CACHE.with(|c| assert!(matches!(c.borrow().get("1 +"), Some(None))));
        assert!(lookup(&"1+".repeat(MAX_CACHED_TEXT)).is_none());
    }
}
