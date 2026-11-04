//! Arithmetic operators and their C semantics, shared by the one-pass
//! evaluator and the cached-tree evaluator so both compute every
//! operator, and report every runtime error, identically.

use super::div_zero_err;

/// A prefix operator: `-x`, `+x`, `!x`, `~x`.
#[derive(Clone, Copy)]
pub(super) enum UnOp {
    Neg,
    Plus,
    Not,
    BitNot,
}

impl UnOp {
    #[inline]
    pub(super) fn apply(self, v: i64) -> i64 {
        match self {
            UnOp::Neg => v.wrapping_neg(),
            UnOp::Plus => v,
            UnOp::Not => i64::from(v == 0),
            UnOp::BitNot => !v,
        }
    }
}

/// A binary operator of the levels between `||`/`&&` and the unary
/// operators (`|` through `%`).
#[derive(Clone, Copy)]
pub(super) enum BinOp {
    BitOr,
    BitXor,
    BitAnd,
    Eq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

impl BinOp {
    /// `left op right`. Division or modulo by zero is an error, except
    /// in an unevaluated operand (`noeval`), where it yields 0.
    #[inline]
    pub(super) fn apply(self, left: i64, right: i64, noeval: bool) -> crate::error::Result<i64> {
        Ok(match self {
            BinOp::BitOr => left | right,
            BinOp::BitXor => left ^ right,
            BinOp::BitAnd => left & right,
            BinOp::Eq => i64::from(left == right),
            BinOp::Ne => i64::from(left != right),
            BinOp::Le => i64::from(left <= right),
            BinOp::Ge => i64::from(left >= right),
            BinOp::Lt => i64::from(left < right),
            BinOp::Gt => i64::from(left > right),
            BinOp::Shl => left.wrapping_shl(right as u32),
            BinOp::Shr => left.wrapping_shr(right as u32),
            BinOp::Add => left.wrapping_add(right),
            BinOp::Sub => left.wrapping_sub(right),
            BinOp::Mul => left.wrapping_mul(right),
            BinOp::Div | BinOp::Mod if right == 0 => {
                if noeval {
                    0
                } else if matches!(self, BinOp::Div) {
                    return Err(div_zero_err("division by zero"));
                } else {
                    return Err(div_zero_err("division by zero (modulo)"));
                }
            }
            // wrapping: INT_MIN / -1 yields INT_MIN (C semantics) instead of panicking
            BinOp::Div => left.wrapping_div(right),
            // wrapping: INT_MIN % -1 yields 0 instead of panicking
            BinOp::Mod => left.wrapping_rem(right),
        })
    }
}

/// The operator of a compound assignment `name op= rhs`.
#[derive(Clone, Copy)]
pub(super) enum CompoundOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    And,
    Xor,
    Or,
}

impl CompoundOp {
    /// The value `name op= rhs` assigns, given the current value `cur`.
    #[inline]
    pub(super) fn apply(self, cur: i64, rhs: i64) -> crate::error::Result<i64> {
        Ok(match self {
            CompoundOp::Add => cur.wrapping_add(rhs),
            CompoundOp::Sub => cur.wrapping_sub(rhs),
            CompoundOp::Mul => cur.wrapping_mul(rhs),
            CompoundOp::Div => {
                if rhs == 0 {
                    return Err(div_zero_err("division by zero"));
                }
                // wrapping: INT_MIN / -1 yields INT_MIN (C semantics)
                cur.wrapping_div(rhs)
            }
            CompoundOp::Mod => {
                if rhs == 0 {
                    return Err(div_zero_err("division by zero (modulo)"));
                }
                // wrapping: INT_MIN % -1 yields 0
                cur.wrapping_rem(rhs)
            }
            CompoundOp::Shl => cur.wrapping_shl(rhs as u32),
            CompoundOp::Shr => cur.wrapping_shr(rhs as u32),
            CompoundOp::And => cur & rhs,
            CompoundOp::Xor => cur ^ rhs,
            CompoundOp::Or => cur | rhs,
        })
    }
}
