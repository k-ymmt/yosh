//! The one-pass evaluator: recursive descent that evaluates while it
//! parses. It is the reference semantics for arithmetic expansion; the
//! cached-tree evaluator ([`super::cache`]) mirrors it.

use super::token::{self, Cursor};
use super::var::{assign, var_operand};
use super::{MAX_NEST_DEPTH, arith_err};
use crate::env::ShellEnv;

/// Recursive-descent arithmetic parser with access to shell environment.
pub(super) struct ArithParser<'a> {
    cur: Cursor<'a>,
    env: &'a mut ShellEnv,
    /// Recursive variable-as-expression evaluation depth (see
    /// `eval_var_value`).
    depth: u32,
    /// Current syntactic nesting depth (see `MAX_NEST_DEPTH`).
    nest: u32,
    /// True while parsing an operand that C semantics leave unevaluated:
    /// the untaken branch of `?:` and the right operand of a
    /// short-circuited `&&` / `||`. The operand is still parsed (syntax
    /// errors are reported) but has no side effects: assignments are not
    /// performed, variables are not evaluated, and division by zero is
    /// not an error (`$((1 ? 2 : (x=5)))` leaves `x` alone; dash/bash).
    noeval: bool,
}

impl<'a> ArithParser<'a> {
    /// A parser for `text` at variable recursion depth `depth` and
    /// syntactic nesting `nest`.
    pub(super) fn new(env: &'a mut ShellEnv, text: &'a str, depth: u32, nest: u32) -> Self {
        ArithParser {
            cur: Cursor::new(text),
            env,
            depth,
            nest,
            noeval: false,
        }
    }

    /// Parse a complete expression and require it to consume all input.
    /// Trailing garbage (`$((1 2))`) is a syntax error, matching bash/dash.
    pub(super) fn eval_full(&mut self) -> crate::error::Result<i64> {
        let val = self.expr()?;
        self.cur.skip_whitespace();
        if !self.cur.at_end() {
            let rest = String::from_utf8_lossy(self.cur.rest());
            return Err(arith_err(format!(
                "syntax error: unexpected token '{}'",
                rest.trim_end()
            )));
        }
        Ok(val)
    }

    /// Parse a nested operand with `parse`: one more level of syntactic
    /// nesting (an arithmetic error instead of a stack overflow past
    /// `MAX_NEST_DEPTH`), in no-evaluation mode when `skip` is set (see
    /// `noeval`; nested skips stay skipped). Every recursive cycle of the
    /// descent — parentheses, unary operators, `?:` branches, assignment
    /// right-hand sides — passes through here.
    fn nested(
        &mut self,
        skip: bool,
        parse: impl FnOnce(&mut Self) -> crate::error::Result<i64>,
    ) -> crate::error::Result<i64> {
        if self.nest >= MAX_NEST_DEPTH || crate::exec::stack_guard::descent_stack_exhausted() {
            return Err(arith_err("expression nested too deeply"));
        }
        self.nest += 1;
        let saved = self.noeval;
        self.noeval |= skip;
        let result = parse(self);
        self.noeval = saved;
        self.nest -= 1;
        result
    }

    fn expect(&mut self, ch: u8) -> crate::error::Result<()> {
        if self.cur.eat(ch) {
            Ok(())
        } else {
            Err(arith_err(format!(
                "expected '{}', got '{}'",
                ch as char,
                self.cur.char_at_pos().unwrap_or('?')
            )))
        }
    }

    // ── Top-level expression ─────────────────────────────────────────────────

    fn expr(&mut self) -> crate::error::Result<i64> {
        self.comma()
    }

    // ── Comma: a, b, c (lowest precedence) ──────────────────────────────────

    fn comma(&mut self) -> crate::error::Result<i64> {
        let mut result = self.ternary()?;
        while self.cur.eat(b',') {
            result = self.ternary()?;
        }
        Ok(result)
    }

    // ── Ternary: a ? b : c ───────────────────────────────────────────────────

    fn ternary(&mut self) -> crate::error::Result<i64> {
        let cond = self.logical_or()?;
        if self.cur.eat(b'?') {
            // Only the selected branch is evaluated (C semantics).
            let then_val = self.nested(cond == 0, Self::ternary)?;
            self.expect(b':')?;
            let else_val = self.nested(cond != 0, Self::ternary)?;
            Ok(if cond != 0 { then_val } else { else_val })
        } else {
            Ok(cond)
        }
    }

    // ── Logical OR: || ───────────────────────────────────────────────────────

    fn logical_or(&mut self) -> crate::error::Result<i64> {
        let mut left = self.logical_and()?;
        while self.cur.eat_pair(*b"||") {
            // Short-circuit: a true left operand leaves the right
            // one unevaluated (no side effects).
            let right = self.nested(left != 0, Self::logical_and)?;
            left = if left != 0 || right != 0 { 1 } else { 0 };
        }
        Ok(left)
    }

    // ── Logical AND: && ──────────────────────────────────────────────────────

    fn logical_and(&mut self) -> crate::error::Result<i64> {
        let mut left = self.bitwise_or()?;
        while self.cur.eat_pair(*b"&&") {
            // Short-circuit: a false left operand leaves the right
            // one unevaluated (no side effects).
            let right = self.nested(left == 0, Self::bitwise_or)?;
            left = if left != 0 && right != 0 { 1 } else { 0 };
        }
        Ok(left)
    }

    // ── Binary levels, loosest first: | ^ & (== !=) (< > <= >=) (<< >>)
    // ── (+ -) (* / %) ────────────────────────────────────────────────────────

    fn bitwise_or(&mut self) -> crate::error::Result<i64> {
        let mut left = self.bitwise_xor()?;
        while let Some(op) = self.cur.binary_op(token::bit_or_op) {
            let right = self.bitwise_xor()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn bitwise_xor(&mut self) -> crate::error::Result<i64> {
        let mut left = self.bitwise_and()?;
        while let Some(op) = self.cur.binary_op(token::bit_xor_op) {
            let right = self.bitwise_and()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn bitwise_and(&mut self) -> crate::error::Result<i64> {
        let mut left = self.equality()?;
        while let Some(op) = self.cur.binary_op(token::bit_and_op) {
            let right = self.equality()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn equality(&mut self) -> crate::error::Result<i64> {
        let mut left = self.relational()?;
        while let Some(op) = self.cur.binary_op(token::equality_op) {
            let right = self.relational()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn relational(&mut self) -> crate::error::Result<i64> {
        let mut left = self.shift()?;
        while let Some(op) = self.cur.binary_op(token::relational_op) {
            let right = self.shift()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn shift(&mut self) -> crate::error::Result<i64> {
        let mut left = self.additive()?;
        while let Some(op) = self.cur.binary_op(token::shift_op) {
            let right = self.additive()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn additive(&mut self) -> crate::error::Result<i64> {
        let mut left = self.multiplicative()?;
        while let Some(op) = self.cur.binary_op(token::additive_op) {
            let right = self.multiplicative()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> crate::error::Result<i64> {
        let mut left = self.unary()?;
        while let Some(op) = self.cur.binary_op(token::multiplicative_op) {
            let right = self.unary()?;
            left = op.apply(left, right, self.noeval)?;
        }
        Ok(left)
    }

    // ── Unary: -, +, !, ~ ───────────────────────────────────────────────────

    fn unary(&mut self) -> crate::error::Result<i64> {
        self.cur.skip_whitespace();
        if self.cur.at_end() {
            return Err(arith_err("unexpected end of expression"));
        }
        match self.cur.unary_op() {
            Some(op) => {
                let v = self.nested(false, Self::unary)?;
                Ok(op.apply(v))
            }
            None => self.primary(),
        }
    }

    // ── Primary: number, variable, (expr) ───────────────────────────────────

    fn primary(&mut self) -> crate::error::Result<i64> {
        self.cur.skip_whitespace();
        let Some(ch) = self.cur.peek() else {
            return Err(arith_err("unexpected end of expression"));
        };

        // Parenthesized expression
        if ch == b'(' {
            self.cur.bump(1);
            let val = self.nested(false, Self::expr)?;
            self.expect(b')')?;
            return Ok(val);
        }

        // Number literal
        if ch.is_ascii_digit() {
            return self.cur.number();
        }

        // Variable name (bare identifier: may also be assignment target)
        if ch.is_ascii_alphabetic() || ch == b'_' {
            return self.parse_name_or_assign();
        }

        Err(arith_err(format!(
            "unexpected character '{}'",
            self.cur.char_at_pos().unwrap_or('?')
        )))
    }

    // ── Identifier: variable lookup or assignment (x = expr) ─────────────────

    fn parse_name_or_assign(&mut self) -> crate::error::Result<i64> {
        let name = self.cur.name()?;

        self.cur.skip_whitespace();

        // Check for compound assignment operators: +=, -=, *=, /=, %=, <<=, >>=, &=, ^=, |=
        if let Some(compound_op) = self.cur.compound_assign_op() {
            let rhs = self.nested(false, Self::ternary)?;
            if self.noeval {
                return Ok(0);
            }
            // Recursively evaluate the current value (`x=1+2; $((x+=1))`
            // is 4 in bash), same as the plain lookup path below.
            let cur_val = self.var_operand(name)?;
            let val = compound_op.apply(cur_val, rhs)?;
            assign(self.env, name, val)?;
            return Ok(val);
        }

        // Check for simple assignment: `name = expr` (not `==`)
        if self.cur.peek() == Some(b'=') && self.cur.peek_at(1) != Some(b'=') {
            self.cur.bump(1); // consume '='
            let val = self.nested(false, Self::ternary)?;
            if !self.noeval {
                assign(self.env, name, val)?;
            }
            return Ok(val);
        }

        // Variable lookup: the value is itself evaluated as an arithmetic
        // expression (POSIX §2.6.4 / C rules; `x=1+2; $((x))` is 3).
        self.var_operand(name)
    }

    fn var_operand(&mut self, name: &str) -> crate::error::Result<i64> {
        var_operand(self.env, name, self.depth, self.nest, self.noeval)
    }
}
