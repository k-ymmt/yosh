use crate::env::ShellEnv;
use crate::error::{ExpansionErrorKind, ShellError};

/// Maximum depth of recursive variable-as-expression evaluation
/// (`x=y; y=1; $((x))`). Prevents infinite loops like `x=x`.
const MAX_RECURSION_DEPTH: u32 = 64;

/// Build the `ShellError` for a general arithmetic evaluation failure.
/// All arithmetic errors are `ShellError::expansion`, so the POSIX §2.8.1
/// consequences table applies uniformly at every call site: a word-context
/// failure aborts a non-interactive shell, a heredoc-context failure is
/// converted to a redirection error at the redirect boundary (matching
/// dash/bash, which keep the shell alive there).
fn arith_err(msg: impl std::fmt::Display) -> ShellError {
    ShellError::expansion(
        ExpansionErrorKind::InvalidArithmetic,
        format!("arithmetic: {}", msg),
    )
}

/// `arith_err` variant for division/modulo by zero, carrying the dedicated
/// `ExpansionErrorKind::DivisionByZero` kind.
fn div_zero_err(msg: &str) -> ShellError {
    ShellError::expansion(
        ExpansionErrorKind::DivisionByZero,
        format!("arithmetic: {}", msg),
    )
}

/// Evaluate an arithmetic expression and return the result as a string.
/// Expands `$VAR`, `${VAR}`, `$(cmd)`, `` `cmd` ``, and nested `$((...))`
/// first (via the shared dollar-scanner), then parses and evaluates.
pub fn evaluate(env: &mut ShellEnv, expr: &str) -> crate::error::Result<String> {
    // Step 1: expand dollar references. Expansions substitute their
    // actual (possibly empty) text — `$((1${x}2))` with `x` unset is 12,
    // matching bash/dash.
    let expanded = super::dollar::expand_string(env, expr)?;

    // An entirely blank expression evaluates to 0 (bash/dash: `$(( ))`
    // and `$(($x))` with `x` unset/empty both print 0).
    if expanded.trim().is_empty() {
        return Ok("0".to_string());
    }

    // Step 2: parse and evaluate
    let bytes = expanded.as_bytes();
    let mut parser = ArithParser {
        input: bytes,
        pos: 0,
        env,
        depth: 0,
    };

    // Callers print the diagnostic (word expansion via the ShellError path
    // in `exec_command`, heredoc expansion via the redirect-error path).
    parser.eval_full().map(|val| val.to_string())
}

/// Look up a bare identifier (`name` in `$((name))`, without a leading
/// `$`) for the tokenizer's variable-reference path. Intercepts `LINENO`
/// as a computed pseudo-variable (see `ExecState::lineno`); unset regular
/// variables default to "0".
fn arith_name_lookup(env: &ShellEnv, name: &str) -> String {
    if name == "LINENO" {
        return env.exec.lineno.to_string();
    }
    env.vars.get(name).unwrap_or("0").to_string()
}

/// Recursive-descent arithmetic parser with access to shell environment.
struct ArithParser<'a> {
    input: &'a [u8],
    pos: usize,
    env: &'a mut ShellEnv,
    /// Recursive variable-as-expression evaluation depth (see
    /// `eval_var_value`).
    depth: u32,
}

impl<'a> ArithParser<'a> {
    /// Parse a complete expression and require it to consume all input.
    /// Trailing garbage (`$((1 2))`) is a syntax error, matching bash/dash.
    fn eval_full(&mut self) -> crate::error::Result<i64> {
        let val = self.expr()?;
        self.skip_whitespace();
        if self.pos < self.input.len() {
            let rest = String::from_utf8_lossy(&self.input[self.pos..]);
            return Err(arith_err(format!(
                "syntax error: unexpected token '{}'",
                rest.trim_end()
            )));
        }
        Ok(val)
    }

    /// Evaluate a variable's raw string value as an operand. A plain
    /// integer parses directly; empty/blank is 0 (matches bash: `x=""`
    /// gives `$((x))` == 0); anything else is recursively evaluated as an
    /// arithmetic expression (`x=1+2; $((x))` == 3, matching bash/dash),
    /// with a depth cap so self-referential values (`x=x`) terminate.
    fn eval_var_value(&mut self, name: &str, raw: &str) -> crate::error::Result<i64> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(0);
        }
        if let Ok(v) = trimmed.parse::<i64>() {
            return Ok(v);
        }
        if self.depth >= MAX_RECURSION_DEPTH {
            return Err(arith_err(format!(
                "{}: expression recursion level exceeded",
                name
            )));
        }
        let mut sub = ArithParser {
            input: trimmed.as_bytes(),
            pos: 0,
            env: self.env,
            depth: self.depth + 1,
        };
        sub.eval_full()
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, ch: u8) -> crate::error::Result<()> {
        self.skip_whitespace();
        if self.pos < self.input.len() && self.input[self.pos] == ch {
            self.pos += 1;
            Ok(())
        } else {
            let got = self.input.get(self.pos).copied().unwrap_or(b'?');
            Err(arith_err(format!(
                "expected '{}', got '{}'",
                ch as char, got as char
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
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b',' {
                self.pos += 1;
                result = self.ternary()?;
            } else {
                break;
            }
        }
        Ok(result)
    }

    // ── Ternary: a ? b : c ───────────────────────────────────────────────────

    fn ternary(&mut self) -> crate::error::Result<i64> {
        let cond = self.logical_or()?;
        self.skip_whitespace();
        if self.pos < self.input.len() && self.input[self.pos] == b'?' {
            self.pos += 1;
            let then_val = self.ternary()?;
            self.expect(b':')?;
            let else_val = self.ternary()?;
            Ok(if cond != 0 { then_val } else { else_val })
        } else {
            Ok(cond)
        }
    }

    // ── Logical OR: || ───────────────────────────────────────────────────────

    fn logical_or(&mut self) -> crate::error::Result<i64> {
        let mut left = self.logical_and()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'|'
                && self.input[self.pos + 1] == b'|'
            {
                self.pos += 2;
                let right = self.logical_and()?;
                left = if left != 0 || right != 0 { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Logical AND: && ──────────────────────────────────────────────────────

    fn logical_and(&mut self) -> crate::error::Result<i64> {
        let mut left = self.bitwise_or()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'&'
                && self.input[self.pos + 1] == b'&'
            {
                self.pos += 2;
                let right = self.bitwise_or()?;
                left = if left != 0 && right != 0 { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Bitwise OR: | ────────────────────────────────────────────────────────

    fn bitwise_or(&mut self) -> crate::error::Result<i64> {
        let mut left = self.bitwise_xor()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len()
                && self.input[self.pos] == b'|'
                && self.input.get(self.pos + 1) != Some(&b'|')
            {
                self.pos += 1;
                let right = self.bitwise_xor()?;
                left |= right;
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Bitwise XOR: ^ ───────────────────────────────────────────────────────

    fn bitwise_xor(&mut self) -> crate::error::Result<i64> {
        let mut left = self.bitwise_and()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b'^' {
                self.pos += 1;
                let right = self.bitwise_and()?;
                left ^= right;
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Bitwise AND: & ───────────────────────────────────────────────────────

    fn bitwise_and(&mut self) -> crate::error::Result<i64> {
        let mut left = self.equality()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len()
                && self.input[self.pos] == b'&'
                && self.input.get(self.pos + 1) != Some(&b'&')
            {
                self.pos += 1;
                let right = self.equality()?;
                left &= right;
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Equality: ==, != ─────────────────────────────────────────────────────

    fn equality(&mut self) -> crate::error::Result<i64> {
        let mut left = self.relational()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'='
                && self.input[self.pos + 1] == b'='
            {
                self.pos += 2;
                let right = self.relational()?;
                left = if left == right { 1 } else { 0 };
            } else if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'!'
                && self.input[self.pos + 1] == b'='
            {
                self.pos += 2;
                let right = self.relational()?;
                left = if left != right { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Relational: <, >, <=, >= ─────────────────────────────────────────────

    fn relational(&mut self) -> crate::error::Result<i64> {
        let mut left = self.shift()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'<'
                && self.input[self.pos + 1] == b'='
            {
                self.pos += 2;
                let right = self.shift()?;
                left = if left <= right { 1 } else { 0 };
            } else if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'>'
                && self.input[self.pos + 1] == b'='
            {
                self.pos += 2;
                let right = self.shift()?;
                left = if left >= right { 1 } else { 0 };
            } else if self.pos < self.input.len()
                && self.input[self.pos] == b'<'
                && self.input.get(self.pos + 1) != Some(&b'<')
            {
                self.pos += 1;
                let right = self.shift()?;
                left = if left < right { 1 } else { 0 };
            } else if self.pos < self.input.len()
                && self.input[self.pos] == b'>'
                && self.input.get(self.pos + 1) != Some(&b'>')
            {
                self.pos += 1;
                let right = self.shift()?;
                left = if left > right { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Shift: <<, >> ────────────────────────────────────────────────────────

    fn shift(&mut self) -> crate::error::Result<i64> {
        let mut left = self.additive()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'<'
                && self.input[self.pos + 1] == b'<'
            {
                self.pos += 2;
                let right = self.additive()?;
                left = left.wrapping_shl(right as u32);
            } else if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'>'
                && self.input[self.pos + 1] == b'>'
            {
                self.pos += 2;
                let right = self.additive()?;
                left = left.wrapping_shr(right as u32);
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Additive: +, - ───────────────────────────────────────────────────────

    fn additive(&mut self) -> crate::error::Result<i64> {
        let mut left = self.multiplicative()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b'+' {
                self.pos += 1;
                let right = self.multiplicative()?;
                left = left.wrapping_add(right);
            } else if self.pos < self.input.len() && self.input[self.pos] == b'-' {
                self.pos += 1;
                let right = self.multiplicative()?;
                left = left.wrapping_sub(right);
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Multiplicative: *, /, % ──────────────────────────────────────────────

    fn multiplicative(&mut self) -> crate::error::Result<i64> {
        let mut left = self.unary()?;
        loop {
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b'*' {
                self.pos += 1;
                let right = self.unary()?;
                left = left.wrapping_mul(right);
            } else if self.pos < self.input.len() && self.input[self.pos] == b'/' {
                self.pos += 1;
                let right = self.unary()?;
                if right == 0 {
                    return Err(div_zero_err("division by zero"));
                }
                // wrapping: INT_MIN / -1 yields INT_MIN (C semantics) instead of panicking
                left = left.wrapping_div(right);
            } else if self.pos < self.input.len() && self.input[self.pos] == b'%' {
                self.pos += 1;
                let right = self.unary()?;
                if right == 0 {
                    return Err(div_zero_err("division by zero (modulo)"));
                }
                // wrapping: INT_MIN % -1 yields 0 instead of panicking
                left = left.wrapping_rem(right);
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Unary: -, +, !, ~ ───────────────────────────────────────────────────

    fn unary(&mut self) -> crate::error::Result<i64> {
        self.skip_whitespace();
        if self.pos < self.input.len() {
            match self.input[self.pos] {
                b'-' => {
                    self.pos += 1;
                    let v = self.unary()?;
                    Ok(v.wrapping_neg())
                }
                b'+' => {
                    self.pos += 1;
                    self.unary()
                }
                b'!' => {
                    self.pos += 1;
                    let v = self.unary()?;
                    Ok(if v == 0 { 1 } else { 0 })
                }
                b'~' => {
                    self.pos += 1;
                    let v = self.unary()?;
                    Ok(!v)
                }
                _ => self.primary(),
            }
        } else {
            Err(arith_err("unexpected end of expression"))
        }
    }

    // ── Primary: number, variable, (expr) ───────────────────────────────────

    fn primary(&mut self) -> crate::error::Result<i64> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Err(arith_err("unexpected end of expression"));
        }

        let ch = self.input[self.pos];

        // Parenthesized expression
        if ch == b'(' {
            self.pos += 1;
            let val = self.expr()?;
            self.expect(b')')?;
            return Ok(val);
        }

        // Number literal
        if ch.is_ascii_digit() {
            return self.parse_number();
        }

        // Variable name (bare identifier: may also be assignment target)
        if ch.is_ascii_alphabetic() || ch == b'_' {
            return self.parse_name_or_assign();
        }

        Err(arith_err(format!("unexpected character '{}'", ch as char)))
    }

    // ── Number literal: decimal, octal (0…), hex (0x…) ──────────────────────

    fn parse_number(&mut self) -> crate::error::Result<i64> {
        let start = self.pos;
        // Collect all digit/letter chars for the number
        while self.pos < self.input.len() && (self.input[self.pos].is_ascii_alphanumeric()) {
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

    // ── Identifier: variable lookup or assignment (x = expr) ─────────────────

    fn parse_name_or_assign(&mut self) -> crate::error::Result<i64> {
        let start = self.pos;
        while self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_alphanumeric() || self.input[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let name = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(arith_err)?
            .to_string();

        self.skip_whitespace();

        // Check for compound assignment operators: +=, -=, *=, /=, %=, <<=, >>=, &=, ^=, |=
        if let Some(compound_op) = self.try_compound_assign_op() {
            let rhs = self.ternary()?;
            let cur = arith_name_lookup(self.env, &name);
            // Recursively evaluate the current value (`x=1+2; $((x+=1))`
            // is 4 in bash), same as the plain lookup path below.
            let cur_val = self.eval_var_value(&name, &cur)?;
            let val = match compound_op {
                CompoundOp::Add => cur_val.wrapping_add(rhs),
                CompoundOp::Sub => cur_val.wrapping_sub(rhs),
                CompoundOp::Mul => cur_val.wrapping_mul(rhs),
                CompoundOp::Div => {
                    if rhs == 0 {
                        return Err(div_zero_err("division by zero"));
                    }
                    // wrapping: INT_MIN / -1 yields INT_MIN (C semantics)
                    cur_val.wrapping_div(rhs)
                }
                CompoundOp::Mod => {
                    if rhs == 0 {
                        return Err(div_zero_err("division by zero (modulo)"));
                    }
                    // wrapping: INT_MIN % -1 yields 0
                    cur_val.wrapping_rem(rhs)
                }
                CompoundOp::Shl => cur_val.wrapping_shl(rhs as u32),
                CompoundOp::Shr => cur_val.wrapping_shr(rhs as u32),
                CompoundOp::And => cur_val & rhs,
                CompoundOp::Xor => cur_val ^ rhs,
                CompoundOp::Or => cur_val | rhs,
            };
            // LINENO is a computed pseudo-variable (see `ExecState::lineno`);
            // an assignment reads back the current line but does not
            // persist (matches bash: `$((LINENO+=1))` does not "stick").
            if name != "LINENO" {
                // assign_var (not vars.set): invalidates the utility hash
                // when PATH is assigned inside arithmetic. Errors
                // (readonly) stay ignored, as before.
                let _ = self.env.assign_var(&name, val.to_string());
            }
            return Ok(val);
        }

        // Check for simple assignment: `name = expr` (not `==`)
        if self.pos < self.input.len()
            && self.input[self.pos] == b'='
            && self.input.get(self.pos + 1) != Some(&b'=')
        {
            self.pos += 1; // consume '='
            let val = self.ternary()?;
            // Assign into env (LINENO: see comment above; assign_var for
            // PATH-cache invalidation, errors stay ignored)
            if name != "LINENO" {
                let _ = self.env.assign_var(&name, val.to_string());
            }
            return Ok(val);
        }

        // Variable lookup: the value is itself evaluated as an arithmetic
        // expression (POSIX §2.6.4 / C rules; `x=1+2; $((x))` is 3).
        let raw = arith_name_lookup(self.env, &name);
        self.eval_var_value(&name, &raw)
    }

    /// Try to match a compound assignment operator at current position.
    /// Returns the operator kind and advances past it (including the `=`), or None.
    fn try_compound_assign_op(&mut self) -> Option<CompoundOp> {
        if self.pos >= self.input.len() {
            return None;
        }
        let ch = self.input[self.pos];
        // Two-character prefix operators: <<= and >>=
        if self.pos + 2 < self.input.len() && self.input[self.pos + 2] == b'=' {
            if ch == b'<' && self.input[self.pos + 1] == b'<' {
                self.pos += 3;
                return Some(CompoundOp::Shl);
            }
            if ch == b'>' && self.input[self.pos + 1] == b'>' {
                self.pos += 3;
                return Some(CompoundOp::Shr);
            }
        }
        // Single-character prefix operators: +=, -=, *=, /=, %=, &=, ^=, |=
        if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'=' {
            let op = match ch {
                b'+' => Some(CompoundOp::Add),
                b'-' => Some(CompoundOp::Sub),
                b'*' => Some(CompoundOp::Mul),
                b'/' => Some(CompoundOp::Div),
                b'%' => Some(CompoundOp::Mod),
                b'&' => Some(CompoundOp::And),
                b'^' => Some(CompoundOp::Xor),
                b'|' => Some(CompoundOp::Or),
                _ => None,
            };
            if op.is_some() {
                self.pos += 2;
            }
            return op;
        }
        None
    }
}

enum CompoundOp {
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

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn test_simple_number() {
        assert_eq!(evaluate(&mut env(), "42"), Ok("42".to_string()));
    }

    // ── PATH cache invalidation through arithmetic assignment (SP2) ──

    #[test]
    fn arith_simple_assignment_to_path_clears_utility_hash() {
        let mut e = env();
        e.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        assert_eq!(evaluate(&mut e, "PATH = 5"), Ok("5".to_string()));
        assert!(e.utility_hash.is_empty());
        assert_eq!(e.vars.get("PATH"), Some("5"));
    }

    #[test]
    fn arith_compound_assignment_to_path_clears_utility_hash() {
        let mut e = env();
        e.vars.set("PATH", "1").unwrap();
        e.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        assert_eq!(evaluate(&mut e, "PATH += 2"), Ok("3".to_string()));
        assert!(e.utility_hash.is_empty());
    }

    #[test]
    fn test_addition() {
        assert_eq!(evaluate(&mut env(), "1 + 2"), Ok("3".to_string()));
    }

    // ── LINENO (computed pseudo-variable, TODO PERF item 1) ──

    #[test]
    fn test_lineno_bare_identifier() {
        let mut e = env();
        e.exec.lineno = 9;
        assert_eq!(evaluate(&mut e, "LINENO"), Ok("9".to_string()));
    }

    #[test]
    fn test_lineno_dollar_form() {
        let mut e = env();
        e.exec.lineno = 9;
        assert_eq!(evaluate(&mut e, "$LINENO"), Ok("9".to_string()));
    }

    #[test]
    fn test_lineno_arith_does_not_persist_assignment() {
        let mut e = env();
        e.exec.lineno = 5;
        assert_eq!(evaluate(&mut e, "LINENO += 1"), Ok("6".to_string()));
        // Must not have materialized a real VarStore entry.
        assert_eq!(e.vars.get("LINENO"), None);
        // The computed pseudo-variable is unaffected by the arithmetic
        // "assignment" (bash: `$((LINENO+=1))` does not stick either).
        assert_eq!(e.exec.lineno, 5);
    }

    #[test]
    fn test_precedence() {
        assert_eq!(evaluate(&mut env(), "2 + 3 * 4"), Ok("14".to_string()));
    }

    #[test]
    fn test_parens() {
        assert_eq!(evaluate(&mut env(), "(2 + 3) * 4"), Ok("20".to_string()));
    }

    #[test]
    fn test_unary_minus() {
        assert_eq!(evaluate(&mut env(), "-5"), Ok("-5".to_string()));
    }

    #[test]
    fn test_comparison() {
        assert_eq!(evaluate(&mut env(), "3 > 2"), Ok("1".to_string()));
    }

    #[test]
    fn test_logical() {
        assert_eq!(evaluate(&mut env(), "1 && 0"), Ok("0".to_string()));
    }

    #[test]
    fn test_ternary() {
        assert_eq!(evaluate(&mut env(), "1 ? 10 : 20"), Ok("10".to_string()));
    }

    #[test]
    fn test_bitwise() {
        assert_eq!(evaluate(&mut env(), "5 & 3"), Ok("1".to_string()));
    }

    #[test]
    fn test_hex() {
        assert_eq!(evaluate(&mut env(), "0xFF"), Ok("255".to_string()));
    }

    #[test]
    fn test_octal() {
        assert_eq!(evaluate(&mut env(), "010"), Ok("8".to_string()));
    }

    #[test]
    fn test_variable() {
        let mut e = env();
        e.vars.set("x", "10").unwrap();
        assert_eq!(evaluate(&mut e, "x + 5"), Ok("15".to_string()));
    }

    #[test]
    fn test_int_min_div_neg_one_wraps() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(
            evaluate(&mut e, "x / -1"),
            Ok("-9223372036854775808".to_string())
        );
    }

    #[test]
    fn test_int_min_mod_neg_one_is_zero() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(evaluate(&mut e, "x % -1"), Ok("0".to_string()));
    }

    #[test]
    fn test_int_min_compound_div_assign_neg_one_wraps() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(
            evaluate(&mut e, "x /= -1"),
            Ok("-9223372036854775808".to_string())
        );
        assert_eq!(e.vars.get("x"), Some("-9223372036854775808"));
    }

    #[test]
    fn test_int_min_compound_mod_assign_neg_one_is_zero() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(evaluate(&mut e, "x %= -1"), Ok("0".to_string()));
        assert_eq!(e.vars.get("x"), Some("0"));
    }

    #[test]
    fn test_dollar_variable() {
        let mut e = env();
        e.vars.set("x", "10").unwrap();
        assert_eq!(evaluate(&mut e, "$x + 5"), Ok("15".to_string()));
    }

    #[test]
    fn test_variable_assign() {
        let mut e = env();
        assert_eq!(evaluate(&mut e, "z = 5 + 3"), Ok("8".to_string()));
        assert_eq!(e.vars.get("z"), Some("8"));
    }

    #[test]
    fn test_positional_param_in_arith() {
        let mut e = ShellEnv::new("yosh", vec!["10".to_string(), "20".to_string()]);
        assert_eq!(evaluate(&mut e, "$1 + $2"), Ok("30".to_string()));
    }

    #[test]
    fn test_positional_param_zero() {
        let mut e = ShellEnv::new("yosh", vec!["5".to_string()]);
        // $0 is the shell name "yosh", non-numeric → defaults to 0
        assert_eq!(evaluate(&mut e, "$0"), Ok("0".to_string()));
    }

    #[test]
    fn test_special_param_hash_in_arith() {
        let mut e = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(evaluate(&mut e, "$# + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_special_param_question_in_arith() {
        let mut e = env();
        e.exec.last_exit_status = 42;
        assert_eq!(evaluate(&mut e, "$?"), Ok("42".to_string()));
    }

    #[test]
    fn test_braced_positional_param_in_arith() {
        let mut e = ShellEnv::new("yosh", vec!["100".to_string()]);
        assert_eq!(evaluate(&mut e, "${1} + 1"), Ok("101".to_string()));
    }

    #[test]
    fn test_unset_positional_param_defaults_to_zero() {
        let mut e = env();
        // No positional params set; $1 should default to 0
        assert_eq!(evaluate(&mut e, "$1 + 5"), Ok("5".to_string()));
    }

    // ── Unified dollar-scanner: full ${...} operator support ──

    #[test]
    fn test_braced_default_operator() {
        let mut e = env();
        assert_eq!(evaluate(&mut e, "${x:-3} + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_braced_length_operator() {
        let mut e = env();
        e.vars.set("a", "hello").unwrap();
        assert_eq!(evaluate(&mut e, "${#a}+1"), Ok("6".to_string()));
    }

    #[test]
    fn test_nested_arith_expansion() {
        let mut e = env();
        assert_eq!(evaluate(&mut e, "$((1+1)) + 1"), Ok("3".to_string()));
    }

    // ── Unified error channel: kind classification ──

    #[test]
    fn test_division_by_zero_has_dedicated_kind() {
        let err = evaluate(&mut env(), "1/0").unwrap_err();
        assert!(
            matches!(
                err.kind,
                crate::error::ShellErrorKind::Expansion(ExpansionErrorKind::DivisionByZero)
            ),
            "got: {:?}",
            err.kind
        );
        assert!(err.message.contains("division by zero"), "got: {err}");
        assert!(err.requires_noninteractive_exit());
    }

    #[test]
    fn test_syntax_error_is_invalid_arithmetic_kind() {
        let err = evaluate(&mut env(), "1 +").unwrap_err();
        assert!(
            matches!(
                err.kind,
                crate::error::ShellErrorKind::Expansion(ExpansionErrorKind::InvalidArithmetic)
            ),
            "got: {:?}",
            err.kind
        );
    }

    // ── Trailing-garbage detection (audit M5) ──

    #[test]
    fn test_trailing_garbage_is_syntax_error() {
        let mut e = env();
        let err = evaluate(&mut e, "1 2").unwrap_err();
        assert!(err.message.contains("syntax error"), "got: {err}");
    }

    #[test]
    fn test_trailing_whitespace_ok() {
        assert_eq!(evaluate(&mut env(), " 1 + 2  \n"), Ok("3".to_string()));
    }

    // ── Recursive variable-as-expression evaluation (audit M5) ──

    #[test]
    fn test_var_value_evaluated_recursively() {
        let mut e = env();
        e.vars.set("x", "1+2").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("3".to_string()));
        assert_eq!(evaluate(&mut e, "x + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_var_value_chain() {
        let mut e = env();
        e.vars.set("x", "y").unwrap();
        e.vars.set("y", "5").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("5".to_string()));
    }

    #[test]
    fn test_self_referential_var_errors_not_hangs() {
        let mut e = env();
        e.vars.set("x", "x").unwrap();
        let err = evaluate(&mut e, "x").unwrap_err();
        assert!(err.message.contains("recursion"), "got: {err}");
    }

    #[test]
    fn test_empty_var_value_is_zero() {
        let mut e = env();
        e.vars.set("x", "").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("0".to_string()));
    }

    #[test]
    fn test_compound_assign_recursive_current_value() {
        let mut e = env();
        e.vars.set("x", "1+2").unwrap();
        assert_eq!(evaluate(&mut e, "x += 1"), Ok("4".to_string()));
        assert_eq!(e.vars.get("x"), Some("4"));
    }
}
