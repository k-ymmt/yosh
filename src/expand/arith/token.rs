//! Lexical scanning of an (already `$`-expanded) arithmetic text, shared
//! by the one-pass evaluator and the cached-tree parser so that both
//! recognize exactly the same tokens.

use super::arith_err;
use super::ops::{BinOp, CompoundOp, UnOp};

/// A byte cursor over the expression text.
pub(super) struct Cursor<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        Cursor {
            input: text.as_bytes(),
            pos: 0,
        }
    }

    pub(super) fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    pub(super) fn peek_at(&self, offset: usize) -> Option<u8> {
        self.input.get(self.pos + offset).copied()
    }

    pub(super) fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// The unconsumed input.
    pub(super) fn rest(&self) -> &'a [u8] {
        &self.input[self.pos..]
    }

    /// Consume `n` bytes already inspected with `peek` / `peek_at`.
    pub(super) fn bump(&mut self, n: usize) {
        self.pos += n;
    }

    pub(super) fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    /// Skip whitespace, then consume `ch` if it comes next.
    pub(super) fn eat(&mut self, ch: u8) -> bool {
        self.skip_whitespace();
        let found = self.peek() == Some(ch);
        if found {
            self.pos += 1;
        }
        found
    }

    /// Skip whitespace, then consume the two-byte operator `op` (`||`,
    /// `&&`) if it comes next.
    pub(super) fn eat_pair(&mut self, op: [u8; 2]) -> bool {
        self.skip_whitespace();
        let found = self.peek() == Some(op[0]) && self.peek_at(1) == Some(op[1]);
        if found {
            self.pos += 2;
        }
        found
    }

    /// Skip whitespace, then consume the operator of one binary
    /// precedence level that `level` (one of the `*_op` matchers below)
    /// recognizes at the cursor.
    pub(super) fn binary_op(
        &mut self,
        level: fn(&Self) -> Option<(BinOp, usize)>,
    ) -> Option<BinOp> {
        self.skip_whitespace();
        let (op, len) = level(self)?;
        self.pos += len;
        Some(op)
    }

    /// Consume a prefix operator (`-` `+` `!` `~`) at the cursor.
    pub(super) fn unary_op(&mut self) -> Option<UnOp> {
        let op = match self.peek()? {
            b'-' => UnOp::Neg,
            b'+' => UnOp::Plus,
            b'!' => UnOp::Not,
            b'~' => UnOp::BitNot,
            _ => return None,
        };
        self.pos += 1;
        Some(op)
    }

    // ── Number literal: decimal, octal (0…), hex (0x…) ──────────────────

    /// Consume an integer constant starting at a digit.
    pub(super) fn number(&mut self) -> crate::error::Result<i64> {
        let start = self.pos;
        // Collect all digit/letter chars for the number
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric()) {
            self.pos += 1;
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).map_err(arith_err)?;

        // Hex
        if s.starts_with("0x") || s.starts_with("0X") {
            i64::from_str_radix(&s[2..], 16)
                .map_err(|e| arith_err(format!("invalid hex literal '{}': {}", s, e)))
        // Octal (leading zero but more digits follow)
        } else if s.starts_with('0') && s.len() > 1 {
            i64::from_str_radix(&s[1..], 8)
                .map_err(|e| arith_err(format!("invalid octal literal '{}': {}", s, e)))
        // Decimal
        } else {
            s.parse::<i64>()
                .map_err(|e| arith_err(format!("invalid number '{}': {}", s, e)))
        }
    }

    /// Consume an identifier starting at a letter or `_`.
    pub(super) fn name(&mut self) -> crate::error::Result<&'a str> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        std::str::from_utf8(&self.input[start..self.pos]).map_err(arith_err)
    }

    /// Try to match a compound assignment operator (`+=`, `<<=`, …) at
    /// the cursor. Returns the operator kind and advances past it
    /// (including the `=`), or None.
    pub(super) fn compound_assign_op(&mut self) -> Option<CompoundOp> {
        let ch = self.peek()?;
        // Two-character prefix operators: <<= and >>=
        if self.peek_at(2) == Some(b'=') {
            let shift = match (ch, self.peek_at(1)) {
                (b'<', Some(b'<')) => Some(CompoundOp::Shl),
                (b'>', Some(b'>')) => Some(CompoundOp::Shr),
                _ => None,
            };
            if shift.is_some() {
                self.pos += 3;
                return shift;
            }
        }
        // Single-character prefix operators: +=, -=, *=, /=, %=, &=, ^=, |=
        if self.peek_at(1) == Some(b'=') {
            let op = match ch {
                b'+' => CompoundOp::Add,
                b'-' => CompoundOp::Sub,
                b'*' => CompoundOp::Mul,
                b'/' => CompoundOp::Div,
                b'%' => CompoundOp::Mod,
                b'&' => CompoundOp::And,
                b'^' => CompoundOp::Xor,
                b'|' => CompoundOp::Or,
                _ => return None,
            };
            self.pos += 2;
            return Some(op);
        }
        None
    }

    /// The whole (possibly multibyte) character at the cursor, for
    /// diagnostics: `$((é+1))` must report `é`, not its first byte.
    pub(super) fn char_at_pos(&self) -> Option<char> {
        let rest = self.input.get(self.pos..)?;
        let head = &rest[..rest.len().min(4)];
        let valid = match std::str::from_utf8(head) {
            Ok(s) => s,
            Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).unwrap_or(""),
        };
        valid.chars().next()
    }
}

// ─── Binary operator matchers, one per precedence level ─────────────────
//
// Each recognizes its level's operator at the cursor (whitespace already
// skipped) without consuming it, returning the operator and its length.
// A single-byte operator that starts a longer one of another level
// (`|` vs `||`, `<` vs `<<`) defers to that level.

pub(super) fn bit_or_op(c: &Cursor) -> Option<(BinOp, usize)> {
    (c.peek() == Some(b'|') && c.peek_at(1) != Some(b'|')).then_some((BinOp::BitOr, 1))
}

pub(super) fn bit_xor_op(c: &Cursor) -> Option<(BinOp, usize)> {
    (c.peek() == Some(b'^')).then_some((BinOp::BitXor, 1))
}

pub(super) fn bit_and_op(c: &Cursor) -> Option<(BinOp, usize)> {
    (c.peek() == Some(b'&') && c.peek_at(1) != Some(b'&')).then_some((BinOp::BitAnd, 1))
}

pub(super) fn equality_op(c: &Cursor) -> Option<(BinOp, usize)> {
    match (c.peek()?, c.peek_at(1)?) {
        (b'=', b'=') => Some((BinOp::Eq, 2)),
        (b'!', b'=') => Some((BinOp::Ne, 2)),
        _ => None,
    }
}

pub(super) fn relational_op(c: &Cursor) -> Option<(BinOp, usize)> {
    match (c.peek()?, c.peek_at(1)) {
        (b'<', Some(b'=')) => Some((BinOp::Le, 2)),
        (b'>', Some(b'=')) => Some((BinOp::Ge, 2)),
        (b'<', next) if next != Some(b'<') => Some((BinOp::Lt, 1)),
        (b'>', next) if next != Some(b'>') => Some((BinOp::Gt, 1)),
        _ => None,
    }
}

pub(super) fn shift_op(c: &Cursor) -> Option<(BinOp, usize)> {
    match (c.peek()?, c.peek_at(1)?) {
        (b'<', b'<') => Some((BinOp::Shl, 2)),
        (b'>', b'>') => Some((BinOp::Shr, 2)),
        _ => None,
    }
}

pub(super) fn additive_op(c: &Cursor) -> Option<(BinOp, usize)> {
    match c.peek()? {
        b'+' => Some((BinOp::Add, 1)),
        b'-' => Some((BinOp::Sub, 1)),
        _ => None,
    }
}

pub(super) fn multiplicative_op(c: &Cursor) -> Option<(BinOp, usize)> {
    match c.peek()? {
        b'*' => Some((BinOp::Mul, 1)),
        b'/' => Some((BinOp::Div, 1)),
        b'%' => Some((BinOp::Mod, 1)),
        _ => None,
    }
}
