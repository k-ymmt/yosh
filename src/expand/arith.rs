use crate::env::ShellEnv;

/// Evaluate an arithmetic expression and return the result as a string.
/// Expands `$VAR` and `${VAR}` first, then parses and evaluates the expression.
pub fn evaluate(env: &mut ShellEnv, expr: &str) -> Result<String, String> {
    // Step 1: expand $VAR and ${VAR} references
    let expanded = expand_vars(env, expr);

    // Step 2: parse and evaluate
    let bytes = expanded.as_bytes();
    let mut parser = ArithParser {
        input: bytes,
        pos: 0,
        env,
    };

    match parser.expr() {
        Ok(val) => Ok(val.to_string()),
        Err(msg) => {
            eprintln!("kish: arithmetic: {}", msg);
            Err(msg)
        }
    }
}

/// Replace `$VAR` and `${VAR}` in an arithmetic expression with their values.
/// Unset variables default to "0".
fn expand_vars(env: &ShellEnv, expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'{' {
                // ${VAR}
                i += 2;
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let name = &expr[start..i];
                if i < bytes.len() {
                    i += 1; // consume '}'
                }
                let val = env.vars.get(name).unwrap_or("0");
                result.push_str(val);
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                // $VAR
                i += 1;
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                let name = &expr[start..i];
                let val = env.vars.get(name).unwrap_or("0");
                result.push_str(val);
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Recursive-descent arithmetic parser with access to shell environment.
struct ArithParser<'a> {
    input: &'a [u8],
    pos: usize,
    env: &'a mut ShellEnv,
}

impl<'a> ArithParser<'a> {
    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, ch: u8) -> Result<(), String> {
        self.skip_whitespace();
        if self.pos < self.input.len() && self.input[self.pos] == ch {
            self.pos += 1;
            Ok(())
        } else {
            let got = self.input.get(self.pos).copied().unwrap_or(b'?');
            Err(format!(
                "expected '{}', got '{}'",
                ch as char, got as char
            ))
        }
    }

    // ── Top-level expression ─────────────────────────────────────────────────

    fn expr(&mut self) -> Result<i64, String> {
        self.ternary()
    }

    // ── Ternary: a ? b : c ───────────────────────────────────────────────────

    fn ternary(&mut self) -> Result<i64, String> {
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

    fn logical_or(&mut self) -> Result<i64, String> {
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

    fn logical_and(&mut self) -> Result<i64, String> {
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

    fn bitwise_or(&mut self) -> Result<i64, String> {
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

    fn bitwise_xor(&mut self) -> Result<i64, String> {
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

    fn bitwise_and(&mut self) -> Result<i64, String> {
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

    fn equality(&mut self) -> Result<i64, String> {
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

    fn relational(&mut self) -> Result<i64, String> {
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

    fn shift(&mut self) -> Result<i64, String> {
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

    fn additive(&mut self) -> Result<i64, String> {
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

    fn multiplicative(&mut self) -> Result<i64, String> {
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
                    return Err("division by zero".to_string());
                }
                left /= right;
            } else if self.pos < self.input.len() && self.input[self.pos] == b'%' {
                self.pos += 1;
                let right = self.unary()?;
                if right == 0 {
                    return Err("division by zero (modulo)".to_string());
                }
                left %= right;
            } else {
                break;
            }
        }
        Ok(left)
    }

    // ── Unary: -, +, !, ~ ───────────────────────────────────────────────────

    fn unary(&mut self) -> Result<i64, String> {
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
            Err("unexpected end of expression".to_string())
        }
    }

    // ── Primary: number, variable, (expr) ───────────────────────────────────

    fn primary(&mut self) -> Result<i64, String> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Err("unexpected end of expression".to_string());
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

        Err(format!("unexpected character '{}'", ch as char))
    }

    // ── Number literal: decimal, octal (0…), hex (0x…) ──────────────────────

    fn parse_number(&mut self) -> Result<i64, String> {
        let start = self.pos;
        // Collect all digit/letter chars for the number
        while self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_alphanumeric())
        {
            self.pos += 1;
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|e| e.to_string())?;

        // Hex
        if s.starts_with("0x") || s.starts_with("0X") {
            i64::from_str_radix(&s[2..], 16)
                .map_err(|e| format!("invalid hex literal '{}': {}", s, e))
        // Octal (leading zero but more digits follow)
        } else if s.starts_with('0') && s.len() > 1 {
            i64::from_str_radix(&s[1..], 8)
                .map_err(|e| format!("invalid octal literal '{}': {}", s, e))
        // Decimal
        } else {
            s.parse::<i64>()
                .map_err(|e| format!("invalid number '{}': {}", s, e))
        }
    }

    // ── Identifier: variable lookup or assignment (x = expr) ─────────────────

    fn parse_name_or_assign(&mut self) -> Result<i64, String> {
        let start = self.pos;
        while self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_alphanumeric() || self.input[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let name = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|e| e.to_string())?
            .to_string();

        self.skip_whitespace();

        // Check for compound assignment operators: +=, -=, *=, /=, %=, <<=, >>=, &=, ^=, |=
        if let Some(compound_op) = self.try_compound_assign_op() {
            let rhs = self.expr()?;
            let cur = self.env.vars.get(&name).unwrap_or("0").to_string();
            let cur_val = cur.trim().parse::<i64>().unwrap_or(0);
            let val = match compound_op {
                CompoundOp::Add => cur_val.wrapping_add(rhs),
                CompoundOp::Sub => cur_val.wrapping_sub(rhs),
                CompoundOp::Mul => cur_val.wrapping_mul(rhs),
                CompoundOp::Div => {
                    if rhs == 0 { return Err("division by zero".to_string()); }
                    cur_val / rhs
                }
                CompoundOp::Mod => {
                    if rhs == 0 { return Err("division by zero".to_string()); }
                    cur_val % rhs
                }
                CompoundOp::Shl => cur_val.wrapping_shl(rhs as u32),
                CompoundOp::Shr => cur_val.wrapping_shr(rhs as u32),
                CompoundOp::And => cur_val & rhs,
                CompoundOp::Xor => cur_val ^ rhs,
                CompoundOp::Or  => cur_val | rhs,
            };
            let _ = self.env.vars.set(&name, val.to_string());
            return Ok(val);
        }

        // Check for simple assignment: `name = expr` (not `==`)
        if self.pos < self.input.len()
            && self.input[self.pos] == b'='
            && self.input.get(self.pos + 1) != Some(&b'=')
        {
            self.pos += 1; // consume '='
            let val = self.expr()?;
            // Assign into env
            let _ = self.env.vars.set(&name, val.to_string());
            return Ok(val);
        }

        // Variable lookup
        let raw = self
            .env
            .vars
            .get(&name)
            .unwrap_or("0")
            .to_string();
        let val = raw.trim().parse::<i64>().unwrap_or(0);
        Ok(val)
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
    Add, Sub, Mul, Div, Mod, Shl, Shr, And, Xor, Or,
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn env() -> ShellEnv {
        ShellEnv::new("kish", vec![])
    }

    #[test]
    fn test_simple_number() {
        assert_eq!(evaluate(&mut env(), "42"), Ok("42".to_string()));
    }

    #[test]
    fn test_addition() {
        assert_eq!(evaluate(&mut env(), "1 + 2"), Ok("3".to_string()));
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
}
