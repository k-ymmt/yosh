//! Floating-point conversion bodies (`%e %f %g %a`) for a finite,
//! non-negative magnitude; the caller adds the sign and padding.

/// `%e`: `d.ddddddE±XX`.
pub(super) fn fmt_e(v: f64, prec: usize, alt: bool, upper: bool) -> String {
    let s = format!("{:.*e}", prec, v);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut out = String::with_capacity(s.len() + 4);
    out.push_str(mant);
    if alt && prec == 0 {
        out.push('.');
    }
    out.push(if upper { 'E' } else { 'e' });
    out.push(if exp < 0 { '-' } else { '+' });
    out.push_str(&format!("{:02}", exp.abs()));
    out
}

/// `%f`.
pub(super) fn fmt_f(v: f64, prec: usize, alt: bool) -> String {
    let mut s = format!("{:.*}", prec, v);
    if alt && prec == 0 {
        s.push('.');
    }
    s
}

/// `%g` per C99 §7.19.6.1.
pub(super) fn fmt_g(v: f64, prec: Option<usize>, alt: bool, upper: bool) -> String {
    let p = match prec {
        Some(0) => 1,
        Some(p) => p,
        None => 6,
    };
    let x: i32 = if v == 0.0 {
        0
    } else {
        let e = format!("{:.*e}", p - 1, v);
        e.split_once('e')
            .and_then(|(_, x)| x.parse().ok())
            .unwrap_or(0)
    };
    let mut s = if x >= -4 && (x as i64) < p as i64 {
        fmt_f(v, (p as i64 - 1 - x as i64).max(0) as usize, alt)
    } else {
        fmt_e(v, p - 1, alt, upper)
    };
    if !alt {
        // Strip trailing zeros in the fraction (and a bare point).
        let (mant, exp) = match s.find(['e', 'E']) {
            Some(i) => (s[..i].to_string(), s[i..].to_string()),
            None => (s.clone(), String::new()),
        };
        let mant = if mant.contains('.') {
            let t = mant.trim_end_matches('0');
            t.trim_end_matches('.').to_string()
        } else {
            mant
        };
        s = mant + &exp;
    }
    s
}

/// `%a`: hexadecimal floating point. Returns (prefix, body) so the `0x`
/// prefix can sit between the sign and zero padding like C does.
pub(super) fn fmt_a(v: f64, prec: Option<usize>, alt: bool, upper: bool) -> (&'static str, String) {
    let bits = v.to_bits();
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    let mut mant = bits & ((1u64 << 52) - 1);
    let (lead, exp) = if exp_bits == 0 {
        if mant == 0 { (0u64, 0) } else { (0u64, -1022) }
    } else {
        (1u64, exp_bits - 1023)
    };
    let mut lead = lead;
    let mut digits: String;
    match prec {
        Some(p) if p < 13 => {
            let drop = (13 - p) * 4;
            let full = (lead << 52) | mant;
            let half = 1u64 << (drop - 1);
            let rem = full & ((1u64 << drop) - 1);
            let mut kept = full >> drop;
            if rem > half || (rem == half && (kept & 1) == 1) {
                kept += 1;
            }
            // A carry out of the fraction makes the leading digit 2
            // (glibc prints `0x2.0p+0`, not a renormalized `0x1.0p+1`).
            lead = kept >> (p * 4);
            mant = kept & ((1u64 << (p * 4)) - 1);
            digits = if p == 0 {
                String::new()
            } else {
                format!("{:0width$x}", mant, width = p)
            };
        }
        Some(p) => {
            digits = format!("{:013x}", mant);
            digits.push_str(&"0".repeat(p - 13));
        }
        None => {
            digits = format!("{:013x}", mant);
            let t = digits.trim_end_matches('0').to_string();
            digits = t;
        }
    }
    let mut body = format!("{}", lead);
    if !digits.is_empty() || alt {
        body.push('.');
        body.push_str(&digits);
    }
    body.push('p');
    body.push(if exp < 0 { '-' } else { '+' });
    body.push_str(&exp.abs().to_string());
    if upper {
        ("0X", body.to_uppercase())
    } else {
        ("0x", body)
    }
}
