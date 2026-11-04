//! Numeric operands: `strto[iu]max` / `strtod`-like parsing and the
//! `State` methods that consume the next operand as a number.

use super::State;
use super::text::first_char_len;

impl State<'_> {
    /// `*` width/precision operand. Values outside C `int` range are
    /// diagnosed and ignored (`None`: the field is treated as unspecified).
    pub(super) fn star_arg(&mut self) -> Option<i32> {
        let Some(a) = self.next_arg() else {
            return Some(0);
        };
        let (v, err) = parse_int(a);
        if let Some(e) = err {
            self.r.diags.push(e);
        }
        if v < i32::MIN as i128 || v > i32::MAX as i128 {
            self.r.diags.push(format!("{}: Result too large", shown(a)));
            return None;
        }
        Some(v as i32)
    }

    /// `%d` / `%i` operand: `strtoimax` semantics — out-of-range values
    /// are clamped with a "Result too large" diagnostic.
    pub(super) fn signed_arg(&mut self) -> i64 {
        let Some(a) = self.next_arg() else { return 0 };
        let (v, err) = parse_int(a);
        if let Some(e) = err {
            self.r.diags.push(e);
        } else if v < i64::MIN as i128 || v > i64::MAX as i128 {
            self.r.diags.push(format!("{}: Result too large", shown(a)));
        }
        v.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// `%u` / `%o` / `%x` operand: `strtoumax` semantics — a negative
    /// value within `-(2^64-1)..=0` wraps modulo 2^64 (`-1` → `u64::MAX`),
    /// anything beyond `±(2^64-1)` is clamped with a diagnostic.
    pub(super) fn unsigned_arg(&mut self) -> u64 {
        let Some(a) = self.next_arg() else { return 0 };
        let (v, err) = parse_int(a);
        if let Some(e) = err {
            self.r.diags.push(e);
        } else if v > u64::MAX as i128 || v < -(u64::MAX as i128) {
            self.r.diags.push(format!("{}: Result too large", shown(a)));
        }
        if v < 0 {
            let m = v.max(-(u64::MAX as i128));
            (m + (1i128 << 64)) as u64
        } else {
            v.min(u64::MAX as i128) as u64
        }
    }

    pub(super) fn float_arg(&mut self) -> f64 {
        let Some(a) = self.next_arg() else { return 0.0 };
        let (v, err) = parse_float(a);
        if let Some(e) = err {
            self.r.diags.push(e);
        }
        v
    }
}

fn shown(a: &[u8]) -> String {
    String::from_utf8_lossy(a).into_owned()
}

/// Value of a `'c` / `"c` operand: the code point of the first character
/// (or the raw byte when it is not valid UTF-8).
fn quoted_char_value(rest: &[u8]) -> i128 {
    let n = first_char_len(rest);
    if n == 0 {
        return 0;
    }
    match std::str::from_utf8(&rest[..n]) {
        Ok(s) => s.chars().next().map(|c| c as u32 as i128).unwrap_or(0),
        Err(_) => rest[0] as i128,
    }
}

/// Parse an integer operand like `strto[iu]max(s, base 0)`.
/// Returns (value, diagnostic). The value is the longest valid prefix,
/// exact while `|value| <= 2^64` (callers apply the conversion's own
/// range rule); beyond that it saturates with a "Result too large"
/// diagnostic.
fn parse_int(a: &[u8]) -> (i128, Option<String>) {
    // An empty operand is 0 without complaint; a blank-only one is not
    // a number (strtoimax consumes nothing).
    if a.is_empty() {
        return (0, None);
    }
    let s = trim_leading_space(a);
    if let Some(&q) = s.first()
        && (q == b'\'' || q == b'"')
    {
        return (quoted_char_value(&s[1..]), None);
    }
    if s.is_empty() {
        return (0, Some(format!("{}: expected numeric value", shown(a))));
    }
    let mut i = 0;
    let mut neg = false;
    match s[0] {
        b'-' => {
            neg = true;
            i = 1;
        }
        b'+' => i = 1,
        _ => {}
    }
    let mut base: u32 = 10;
    if s.len() > i + 1 && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
        // Only treat as hex if a hex digit follows; else "0" then junk.
        if s.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit()) {
            base = 16;
            i += 2;
        }
    } else if s.len() > i && s[i] == b'0' {
        base = 8;
    }
    let start = i;
    let mut v: i128 = 0;
    let mut overflow = false;
    while let Some(&c) = s.get(i) {
        let Some(d) = (c as char).to_digit(base) else {
            break;
        };
        if !overflow {
            v = v * base as i128 + d as i128;
            if v > (u64::MAX as i128) * 2 {
                overflow = true;
            }
        }
        i += 1;
    }
    if i == start {
        return (0, Some(format!("{}: expected numeric value", shown(a))));
    }
    if neg {
        v = -v;
    }
    if overflow {
        let clamped = if neg {
            i64::MIN as i128
        } else {
            u64::MAX as i128
        };
        return (clamped, Some(format!("{}: Result too large", shown(a))));
    }
    if i != s.len() {
        return (v, Some(format!("{}: not completely converted", shown(a))));
    }
    (v, None)
}

/// Parse a floating-point operand like `strtod`.
fn parse_float(a: &[u8]) -> (f64, Option<String>) {
    if a.is_empty() {
        return (0.0, None);
    }
    let s = trim_leading_space(a);
    if let Some(&q) = s.first()
        && (q == b'\'' || q == b'"')
    {
        return (quoted_char_value(&s[1..]) as f64, None);
    }
    if s.is_empty() {
        return (0.0, Some(format!("{}: expected numeric value", shown(a))));
    }
    let text = String::from_utf8_lossy(s);
    let bytes = text.as_bytes();
    let mut i = 0;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    if bytes.len() > i + 1
        && bytes[i] == b'0'
        && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X')
        && let Some((v, end)) = parse_hex_float(bytes, i + 2)
    {
        let v = if bytes[0] == b'-' { -v } else { v };
        if end != bytes.len() {
            return (v, Some(format!("{}: not completely converted", shown(a))));
        }
        return (v, None);
    }
    let lower = text[i..].to_ascii_lowercase();
    let word_len = if lower.starts_with("infinity") {
        8
    } else if lower.starts_with("inf") {
        3
    } else if let Some(rest) = lower.strip_prefix("nan") {
        // strtod also accepts `nan(n-char-sequence)`.
        let rest = rest.as_bytes();
        let close = rest.iter().position(|c| *c == b')');
        match (rest.first(), close) {
            (Some(b'('), Some(k))
                if rest[1..k]
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || *c == b'_') =>
            {
                3 + k + 1
            }
            _ => 3,
        }
    } else {
        0
    };
    let end = if word_len > 0 {
        i + word_len
    } else {
        let mut j = i;
        let mut saw_digit = false;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
            saw_digit = true;
        }
        if j < bytes.len() && bytes[j] == b'.' {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
                saw_digit = true;
            }
        }
        if !saw_digit {
            return (0.0, Some(format!("{}: expected numeric value", shown(a))));
        }
        if j < bytes.len() && (bytes[j] == b'e' || bytes[j] == b'E') {
            let mut k = j + 1;
            if k < bytes.len() && (bytes[k] == b'+' || bytes[k] == b'-') {
                k += 1;
            }
            let ds = k;
            while k < bytes.len() && bytes[k].is_ascii_digit() {
                k += 1;
            }
            if k > ds {
                j = k;
            }
        }
        j
    };
    let v: f64 = if word_len > 3 {
        f64::NAN
    } else {
        text[..end].parse().unwrap_or(0.0)
    };
    if v.is_infinite() && word_len == 0 {
        return (v, Some(format!("{}: Result too large", shown(a))));
    }
    if end != bytes.len() {
        return (v, Some(format!("{}: not completely converted", shown(a))));
    }
    (v, None)
}

/// Parse the part of a C99 hexadecimal float after `0x`: hex digits with
/// an optional point and an optional binary exponent (`p±ddd`). Returns
/// the magnitude and the index just past the parsed text, or `None`
/// when no hex digit was present.
fn parse_hex_float(b: &[u8], mut i: usize) -> Option<(f64, usize)> {
    // Accumulate at most 16 significant hex digits (64 bits, more than
    // f64's 53) so a long operand cannot overflow the mantissa; extra
    // integer digits only raise the binary exponent, extra fraction
    // digits are dropped (like strtod's rounding of excess digits).
    const MAX_SIG: u32 = 16;
    let mut mant: f64 = 0.0;
    let mut exp: i32 = 0;
    let mut digits = 0;
    let mut sig = 0;
    while let Some(d) = b.get(i).and_then(|c| (*c as char).to_digit(16)) {
        if sig < MAX_SIG {
            mant = mant * 16.0 + d as f64;
            if mant != 0.0 {
                sig += 1;
            }
        } else {
            exp = exp.saturating_add(4);
        }
        i += 1;
        digits += 1;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        while let Some(d) = b.get(i).and_then(|c| (*c as char).to_digit(16)) {
            if sig < MAX_SIG {
                mant = mant * 16.0 + d as f64;
                exp -= 4;
                if mant != 0.0 {
                    sig += 1;
                }
            }
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'p') | Some(b'P')) {
        let mut j = i + 1;
        let neg = match b.get(j) {
            Some(b'-') => {
                j += 1;
                true
            }
            Some(b'+') => {
                j += 1;
                false
            }
            _ => false,
        };
        let start = j;
        let mut e: i32 = 0;
        while let Some(c) = b.get(j).filter(|c| c.is_ascii_digit()) {
            e = e.saturating_mul(10).saturating_add((c - b'0') as i32);
            j += 1;
        }
        if j > start {
            exp = exp.saturating_add(if neg { -e } else { e });
            i = j;
        }
    }
    Some((scale_pow2(mant, exp), i))
}

/// `mant * 2^exp` without intermediate overflow/underflow: `powi`
/// alone underflows to 0 for exponents below -1022 (`0x1p-1074`) and
/// makes `0 * inf` for huge positive exponents (`0x0p1024`).
fn scale_pow2(mant: f64, mut exp: i32) -> f64 {
    if mant == 0.0 {
        return 0.0;
    }
    let mut v = mant;
    while exp > 1000 {
        v *= 2f64.powi(1000);
        exp -= 1000;
        if v.is_infinite() {
            return v;
        }
    }
    while exp < -1000 {
        v *= 2f64.powi(-1000);
        exp += 1000;
        if v == 0.0 {
            return 0.0;
        }
    }
    v * 2f64.powi(exp)
}

fn trim_leading_space(a: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < a.len() && matches!(a[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    &a[i..]
}
