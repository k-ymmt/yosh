//! `printf` regular builtin (POSIX XCU `printf`).
//!
//! Implemented natively so that output-heavy scripts do not pay a
//! fork+exec per call (measured 2026-09-16: `/usr/bin/printf` dispatch
//! cost ~1.3 ms per invocation, making `printf` loops ~300× slower than
//! dash). Semantics follow POSIX XCU `printf` plus XBD §5 File Format
//! Notation:
//!
//! - Format escapes: `\\ \a \b \f \n \r \t \v \ddd` (1–3 octal digits).
//! - Conversions: `d i o u x X c s b e E f F g G a A %`.
//! - Flags `- + space # 0`, width and precision (numeric or `*`).
//! - `%b` arguments interpret the same escapes plus `\0ddd` and `\c`
//!   (stop all output).
//! - The format is reused while arguments remain; missing string
//!   arguments are empty, missing numeric arguments are zero.
//! - Numeric arguments accept a leading `'c` / `"c` (character value),
//!   `0x` hex and leading-`0` octal integers. Malformed numbers produce
//!   a diagnostic, use the parsed prefix, and make the exit status 1.
//!
//! Arguments arrive byteenc-encoded; every operand is decoded to raw
//! bytes on entry and the assembled output is written to stdout as raw
//! bytes, so invalid-UTF-8 input round-trips unchanged.

use crate::byteenc;
use crate::error::ShellError;

/// Outcome of one `printf` invocation before it touches stdout/stderr.
struct Run {
    out: Vec<u8>,
    diags: Vec<String>,
    /// Usage error (exit 2) rather than a conversion error (exit 1).
    usage: bool,
}

pub fn builtin_printf(args: &[String]) -> Result<i32, ShellError> {
    let mut run = run(args);
    if !run.out.is_empty()
        && let Err(e) = write_all_stdout(&run.out)
    {
        run.diags.push(format!("write error: {}", e));
    }
    for d in &run.diags {
        eprintln!("yosh: printf: {}", d);
    }
    Ok(if run.usage {
        2
    } else if run.diags.is_empty() {
        0
    } else {
        1
    })
}

/// Write `buf` to fd 1 with `write(2)` directly. `std::io::Stdout`
/// silently swallows `EBADF` (a closed stdout, `printf x >&-`), but
/// POSIX requires `printf` to report a write failure with status 1.
fn write_all_stdout(buf: &[u8]) -> std::io::Result<()> {
    let _lock = std::io::stdout().lock();
    let mut rest = buf;
    while !rest.is_empty() {
        let n = unsafe { libc::write(1, rest.as_ptr().cast(), rest.len()) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        rest = &rest[n as usize..];
    }
    Ok(())
}

/// Evaluate `printf` over byteenc-encoded `args` without performing I/O.
fn run(args: &[String]) -> Run {
    let mut r = Run {
        out: Vec::new(),
        diags: Vec::new(),
        usage: false,
    };
    let args: &[String] = match args.first().map(String::as_str) {
        Some("--") => &args[1..],
        _ => args,
    };
    let Some(format) = args.first() else {
        r.diags
            .push("usage: printf format [argument...]".to_string());
        r.usage = true;
        return r;
    };
    let format = byteenc::decode_bytes(format);
    let operands: Vec<std::borrow::Cow<'_, [u8]>> =
        args[1..].iter().map(|a| byteenc::decode_bytes(a)).collect();
    let mut st = State {
        args: &operands,
        next: 0,
        r: &mut r,
    };
    loop {
        let consumed_before = st.next;
        match st.pass(&format) {
            Pass::Stop => break,
            Pass::Done => {}
        }
        // POSIX: reuse the format while operands remain, but only when
        // the format actually consumes operands (else we would loop).
        if st.next >= st.args.len() || st.next == consumed_before {
            break;
        }
    }
    r
}

enum Pass {
    /// Finished the format normally.
    Done,
    /// `\c` was seen or a fatal format error occurred: stop all output.
    Stop,
}

struct State<'a> {
    args: &'a [std::borrow::Cow<'a, [u8]>],
    next: usize,
    r: &'a mut Run,
}

#[derive(Default, Clone, Copy)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: Option<usize>,
    prec: Option<usize>,
}

impl<'a> State<'a> {
    fn next_arg(&mut self) -> Option<&'a [u8]> {
        let a = self.args.get(self.next)?;
        self.next += 1;
        Some(a)
    }

    fn pass(&mut self, fmt: &[u8]) -> Pass {
        let mut i = 0;
        while i < fmt.len() {
            match fmt[i] {
                b'\\' => {
                    let (consumed, stop) = unescape_into(&fmt[i..], false, &mut self.r.out);
                    debug_assert!(!stop);
                    i += consumed;
                }
                b'%' => match self.directive(fmt, i) {
                    Ok(Some(n)) => i = n,
                    Ok(None) => return Pass::Stop,
                    Err(msg) => {
                        self.r.diags.push(msg);
                        return Pass::Stop;
                    }
                },
                c => {
                    self.r.out.push(c);
                    i += 1;
                }
            }
        }
        Pass::Done
    }

    /// Parse and apply the directive starting at `fmt[start] == b'%'`.
    /// Returns the index just past it, `Ok(None)` when `\c` stopped
    /// output, or an error message for a malformed directive.
    fn directive(&mut self, fmt: &[u8], start: usize) -> Result<Option<usize>, String> {
        let mut i = start + 1;
        let mut spec = Spec::default();
        if fmt.get(i) == Some(&b'%') {
            self.r.out.push(b'%');
            return Ok(Some(i + 1));
        }
        while let Some(&c) = fmt.get(i) {
            match c {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.alt = true,
                b'0' => spec.zero = true,
                _ => break,
            }
            i += 1;
        }
        // Width and precision are C `int`s: values outside that range
        // are diagnosed ("Result too large", like bash) and ignored
        // instead of being turned into a multi-gigabyte allocation.
        if fmt.get(i) == Some(&b'*') {
            i += 1;
            match self.star_arg() {
                Some(w) if w < 0 => {
                    spec.left = true;
                    spec.width = Some(w.unsigned_abs() as usize);
                }
                Some(w) => spec.width = Some(w as usize),
                None => {}
            }
        } else {
            let (n, len) = digits(&fmt[i..]);
            if len > 0 {
                if n > i32::MAX as usize {
                    let shown = String::from_utf8_lossy(&fmt[start..i + len]).into_owned();
                    return Err(format!("{}: field width too large", shown));
                }
                spec.width = Some(n);
            }
            i += len;
        }
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            if fmt.get(i) == Some(&b'*') {
                i += 1;
                spec.prec = match self.star_arg() {
                    Some(p) if p >= 0 => Some(p as usize),
                    _ => None,
                };
            } else {
                let (n, len) = digits(&fmt[i..]);
                if n > i32::MAX as usize {
                    let shown = String::from_utf8_lossy(&fmt[start..i + len]).into_owned();
                    return Err(format!("{}: precision too large", shown));
                }
                spec.prec = Some(n);
                i += len;
            }
        }
        let Some(&conv) = fmt.get(i) else {
            return Err("missing format character".to_string());
        };
        i += 1;
        match conv {
            b'd' | b'i' => {
                let v = self.signed_arg();
                let body = v.unsigned_abs().to_string();
                self.emit_number(
                    &spec,
                    v < 0,
                    "",
                    body,
                    NumKind::Int {
                        octal_alt: false,
                        signed: true,
                    },
                );
            }
            b'u' => {
                let v = self.unsigned_arg();
                self.emit_number(
                    &spec,
                    false,
                    "",
                    v.to_string(),
                    NumKind::Int {
                        octal_alt: false,
                        signed: false,
                    },
                );
            }
            b'o' => {
                let v = self.unsigned_arg();
                let mut body = format!("{:o}", v);
                if spec.alt && !body.starts_with('0') {
                    body.insert(0, '0');
                }
                self.emit_number(
                    &spec,
                    false,
                    "",
                    body,
                    NumKind::Int {
                        octal_alt: spec.alt,
                        signed: false,
                    },
                );
            }
            b'x' | b'X' => {
                let v = self.unsigned_arg();
                let body = if conv == b'x' {
                    format!("{:x}", v)
                } else {
                    format!("{:X}", v)
                };
                let prefix = match (spec.alt && v != 0, conv) {
                    (true, b'x') => "0x",
                    (true, _) => "0X",
                    _ => "",
                };
                self.emit_number(
                    &spec,
                    false,
                    prefix,
                    body,
                    NumKind::Int {
                        octal_alt: false,
                        signed: false,
                    },
                );
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
                let v = self.float_arg();
                let upper = conv.is_ascii_uppercase();
                let neg = v.is_sign_negative() && !v.is_nan();
                let mag = v.abs();
                let (prefix, body, kind) = if !mag.is_finite() {
                    let s = if mag.is_nan() { "nan" } else { "inf" };
                    let s = if upper {
                        s.to_uppercase()
                    } else {
                        s.to_string()
                    };
                    ("", s, NumKind::NonFinite)
                } else {
                    match conv.to_ascii_lowercase() {
                        b'e' => (
                            "",
                            fmt_e(mag, spec.prec.unwrap_or(6), spec.alt, upper),
                            NumKind::Float,
                        ),
                        b'f' => (
                            "",
                            fmt_f(mag, spec.prec.unwrap_or(6), spec.alt),
                            NumKind::Float,
                        ),
                        b'g' => ("", fmt_g(mag, spec.prec, spec.alt, upper), NumKind::Float),
                        _ => {
                            let (p, b) = fmt_a(mag, spec.prec, spec.alt, upper);
                            (p, b, NumKind::Float)
                        }
                    }
                };
                self.emit_number(&spec, neg, prefix, body, kind);
            }
            b'c' => {
                let a = self.next_arg().unwrap_or(b"");
                let first = first_char_len(a);
                let body = a[..first].to_vec();
                self.emit_padded(&spec, body);
            }
            b's' => {
                let a = self.next_arg().unwrap_or(b"");
                let body = match spec.prec {
                    Some(p) => truncate_chars(a, p).to_vec(),
                    None => a.to_vec(),
                };
                self.emit_padded(&spec, body);
            }
            b'b' => {
                let a = self.next_arg().unwrap_or(b"");
                let mut expanded = Vec::with_capacity(a.len());
                let mut j = 0;
                let mut stop = false;
                while j < a.len() {
                    if a[j] == b'\\' {
                        let (n, s) = unescape_into(&a[j..], true, &mut expanded);
                        j += n;
                        if s {
                            stop = true;
                            break;
                        }
                    } else {
                        expanded.push(a[j]);
                        j += 1;
                    }
                }
                let body = match spec.prec {
                    Some(p) => truncate_chars(&expanded, p).to_vec(),
                    None => expanded,
                };
                self.emit_padded(&spec, body);
                if stop {
                    return Ok(None);
                }
            }
            _ => {
                let shown = String::from_utf8_lossy(&fmt[start..i]).into_owned();
                return Err(format!("{}: invalid directive", shown));
            }
        }
        Ok(Some(i))
    }

    /// `*` width/precision operand. Values outside C `int` range are
    /// diagnosed and ignored (`None`: the field is treated as unspecified).
    fn star_arg(&mut self) -> Option<i32> {
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
    fn signed_arg(&mut self) -> i64 {
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
    fn unsigned_arg(&mut self) -> u64 {
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

    fn float_arg(&mut self) -> f64 {
        let Some(a) = self.next_arg() else { return 0.0 };
        let (v, err) = parse_float(a);
        if let Some(e) = err {
            self.r.diags.push(e);
        }
        v
    }

    /// Emit a numeric conversion: `sign prefix [zeros] body`, honouring
    /// precision (minimum digits, integers only), `0` flag and width.
    fn emit_number(&mut self, spec: &Spec, neg: bool, prefix: &str, body: String, kind: NumKind) {
        let signed = !matches!(kind, NumKind::Int { signed: false, .. });
        let sign: &str = if neg {
            "-"
        } else if spec.plus && signed {
            "+"
        } else if spec.space && signed {
            " "
        } else {
            ""
        };
        let mut digits = body;
        // C: the 0 flag pads finite numbers only, and is ignored for
        // integers when a precision is given.
        let mut zero_ok = spec.zero && !spec.left && !matches!(kind, NumKind::NonFinite);
        if let NumKind::Int { octal_alt, .. } = kind
            && let Some(p) = spec.prec
        {
            if p == 0 && digits == "0" {
                // `%.0d` of 0 prints nothing; `%#.0o` still prints "0".
                if !octal_alt {
                    digits.clear();
                }
            } else if digits.len() < p {
                let pad = p - digits.len();
                digits.insert_str(0, &"0".repeat(pad));
            }
            zero_ok = false;
        }
        let len = sign.len() + prefix.len() + digits.len();
        let width = spec.width.unwrap_or(0);
        let out = &mut self.r.out;
        if width <= len {
            out.extend_from_slice(sign.as_bytes());
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(digits.as_bytes());
        } else if spec.left {
            out.extend_from_slice(sign.as_bytes());
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(digits.as_bytes());
            out.resize(out.len() + (width - len), b' ');
        } else if zero_ok {
            out.extend_from_slice(sign.as_bytes());
            out.extend_from_slice(prefix.as_bytes());
            out.resize(out.len() + (width - len), b'0');
            out.extend_from_slice(digits.as_bytes());
        } else {
            out.resize(out.len() + (width - len), b' ');
            out.extend_from_slice(sign.as_bytes());
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(digits.as_bytes());
        }
    }

    /// Emit a string-like conversion (`%s`, `%c`, `%b`) with width padding.
    fn emit_padded(&mut self, spec: &Spec, body: Vec<u8>) {
        let len = char_len(&body);
        let width = spec.width.unwrap_or(0);
        let out = &mut self.r.out;
        if width <= len {
            out.extend_from_slice(&body);
        } else if spec.left {
            out.extend_from_slice(&body);
            out.resize(out.len() + (width - len), b' ');
        } else {
            out.resize(out.len() + (width - len), b' ');
            out.extend_from_slice(&body);
        }
    }
}

#[derive(Clone, Copy)]
enum NumKind {
    /// Integer conversion; `octal_alt` marks `%#o` (0 keeps its digit),
    /// `signed` whether the `+` / space flags apply (`%d` / `%i` only).
    Int {
        octal_alt: bool,
        signed: bool,
    },
    Float,
    NonFinite,
}

/// Parse a run of ASCII digits; returns (value, bytes consumed).
fn digits(s: &[u8]) -> (usize, usize) {
    let mut n: usize = 0;
    let mut i = 0;
    while let Some(&c) = s.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((c - b'0') as usize);
        i += 1;
    }
    (n, i)
}

/// Decode one backslash escape at `s[0] == b'\\'` into `out`.
/// Returns (bytes consumed, stop) where `stop` is `\c` in `%b` mode.
fn unescape_into(s: &[u8], b_mode: bool, out: &mut Vec<u8>) -> (usize, bool) {
    let Some(&c) = s.get(1) else {
        out.push(b'\\');
        return (1, false);
    };
    let simple = match c {
        b'\\' => Some(b'\\'),
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        _ => None,
    };
    if let Some(b) = simple {
        out.push(b);
        return (2, false);
    }
    if b_mode && c == b'c' {
        return (2, true);
    }
    if c.is_ascii_digit() && c < b'8' {
        // Format: \ddd (1-3 octal digits). %b: \0ddd (0 then up to 3
        // digits); a non-zero leading digit is accepted as \ddd too.
        let mut i = 1;
        let mut max_digits = 3;
        if b_mode && c == b'0' {
            i = 2;
            max_digits = 3;
            // If nothing follows the 0, the escape is just NUL.
        }
        let mut v: u32 = 0;
        let mut n = 0;
        while n < max_digits {
            match s.get(i) {
                Some(&d) if (b'0'..=b'7').contains(&d) => {
                    v = v * 8 + (d - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                _ => break,
            }
        }
        if b_mode && c == b'0' && n == 0 {
            out.push(0);
            return (2, false);
        }
        out.push((v & 0xff) as u8);
        return (i, false);
    }
    // Unknown escape: emit the backslash and let the next byte be
    // processed literally by the caller.
    out.push(b'\\');
    (1, false)
}

/// Length in bytes of the first character of `s` (a whole UTF-8
/// sequence when valid, otherwise a single byte); 0 for empty input.
fn first_char_len(s: &[u8]) -> usize {
    if s.is_empty() {
        return 0;
    }
    let want = match s[0] {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return 1,
    };
    if s.len() >= want && std::str::from_utf8(&s[..want]).is_ok() {
        want
    } else {
        1
    }
}

/// Number of characters in `s` counting each invalid byte as one.
fn char_len(s: &[u8]) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < s.len() {
        i += first_char_len(&s[i..]);
        n += 1;
    }
    n
}

/// First `n` characters of `s` (see [`char_len`]).
fn truncate_chars(s: &[u8], n: usize) -> &[u8] {
    let mut i = 0;
    let mut k = 0;
    while i < s.len() && k < n {
        i += first_char_len(&s[i..]);
        k += 1;
    }
    &s[..i]
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

/// `%e`: `d.ddddddE±XX`.
fn fmt_e(v: f64, prec: usize, alt: bool, upper: bool) -> String {
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
fn fmt_f(v: f64, prec: usize, alt: bool) -> String {
    let mut s = format!("{:.*}", prec, v);
    if alt && prec == 0 {
        s.push('.');
    }
    s
}

/// `%g` per C99 §7.19.6.1.
fn fmt_g(v: f64, prec: Option<usize>, alt: bool, upper: bool) -> String {
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
fn fmt_a(v: f64, prec: Option<usize>, alt: bool, upper: bool) -> (&'static str, String) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pf(args: &[&str]) -> (String, i32, Vec<String>) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let r = run(&args);
        let status = if r.usage {
            2
        } else if r.diags.is_empty() {
            0
        } else {
            1
        };
        (
            String::from_utf8_lossy(&r.out).into_owned(),
            status,
            r.diags,
        )
    }

    fn out(args: &[&str]) -> String {
        pf(args).0
    }

    #[test]
    fn literal_and_escapes() {
        assert_eq!(out(&["a\\tb\\n"]), "a\tb\n");
        assert_eq!(out(&["\\101\\x41\\n"]), "A\\x41\n");
        assert_eq!(out(&["\\\\\\a\\b\\f\\r\\v"]), "\\\x07\x08\x0c\r\x0b");
        assert_eq!(out(&["trailing\\"]), "trailing\\");
        assert_eq!(out(&["%%|%5%"]), "%|"); // second is an invalid directive
        assert_eq!(out(&["100%%\\n"]), "100%\n");
    }

    #[test]
    fn strings_and_chars() {
        assert_eq!(out(&["%s|%s\\n", "a", "b"]), "a|b\n");
        assert_eq!(out(&["%5.2s|%-5s|", "hello", "ab"]), "   he|ab   |");
        assert_eq!(out(&["%c%c|%5c|", "hello", "", "x"]), "h|    x|");
        assert_eq!(out(&["%c", "日本"]), "日");
        assert_eq!(out(&["%.1s|%3s|", "日本", "日"]), "日|  日|");
        assert_eq!(out(&["%s\\n"]), "\n");
    }

    #[test]
    fn format_reuse() {
        assert_eq!(out(&["%s %s\\n", "a", "b", "c"]), "a b\nc \n");
        assert_eq!(out(&["%d|", "1", "2", "3"]), "1|2|3|");
        assert_eq!(out(&["x\\n", "a", "b"]), "x\n");
    }

    #[test]
    fn integers() {
        assert_eq!(out(&["%d|%i|%d", "5", "-7", "+3"]), "5|-7|3");
        assert_eq!(out(&["%d|%s|", "5"]), "5||");
        assert_eq!(out(&["%d", ""]), "0");
        assert_eq!(out(&["%i %d", "0x1f", "010"]), "31 8");
        assert_eq!(
            out(&["%u %x %o", "-1", "-1", "-1"]),
            "18446744073709551615 ffffffffffffffff 1777777777777777777777"
        );
        assert_eq!(
            out(&["%X %#x %#X %#o %#o", "255", "255", "255", "8", "0"]),
            "FF 0xff 0XFF 010 0"
        );
        assert_eq!(out(&["%.4x|%#.0o|%#.0x|", "30", "0", "0"]), "001e|0||");
        assert_eq!(
            out(&["%05d|%+d|% d|%-5d|%5d|", "42", "42", "42", "42", "42"]),
            "00042|+42| 42|42   |   42|"
        );
        assert_eq!(
            out(&["%.3d|%.0d|%5.3d|%05.3d|", "7", "0", "7", "7"]),
            "007||  007|  007|"
        );
        assert_eq!(out(&["%d", "'A"]), "65");
        assert_eq!(out(&["%d", "\"€"]), "8364");
        assert_eq!(out(&["%d", " 12"]), "12");
        assert_eq!(
            out(&["%+u|% x|%+o|%+d|% i", "12", "12", "12", "12", "12"]),
            "12|c|14|+12| 12"
        );
        let (o, st, d) = pf(&["%d|%f|", " ", "\t"]);
        assert_eq!((o.as_str(), st), ("0|0.000000|", 1));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn integer_errors() {
        let (o, st, d) = pf(&["%d\\n", "12abc"]);
        assert_eq!(o, "12\n");
        assert_eq!(st, 1);
        assert_eq!(d, vec!["12abc: not completely converted"]);
        let (o, st, d) = pf(&["%d\\n", "abc"]);
        assert_eq!(o, "0\n");
        assert_eq!(st, 1);
        assert_eq!(d, vec!["abc: expected numeric value"]);
        let (o, st, _) = pf(&["%d\\n", "99999999999999999999"]);
        assert_eq!(o, "9223372036854775807\n");
        assert_eq!(st, 1);
        let (o, _, _) = pf(&["%u\\n", "18446744073709551615"]);
        assert_eq!(o, "18446744073709551615\n");
    }

    #[test]
    fn star_width_precision() {
        assert_eq!(
            out(&["%*d|%-*d|%.*f", "5", "3", "5", "3", "2", "3.14159"]),
            "    3|3    |3.14"
        );
        assert_eq!(out(&["%*d|", "-4", "3"]), "3   |");
        // Out-of-int-range `*` operands are diagnosed and ignored, never
        // allocated (a 99999999999-column pad hung the shell before).
        let (o, st, d) = pf(&["%*d|%.*s|", "99999999999", "1", "-99999999999", "abc"]);
        assert_eq!(o, "1|abc|");
        assert_eq!(st, 1);
        assert_eq!(d.len(), 2);
        assert!(d[0].contains("Result too large"));
        let (o, st, d) = pf(&["%18446744073709551616s|", "x"]);
        assert_eq!((o.as_str(), st), ("", 1));
        assert!(d[0].contains("field width too large"), "{d:?}");
        let (o, st, d) = pf(&["%99999999999d|", "1"]);
        assert_eq!((o.as_str(), st), ("", 1));
        assert!(d[0].contains("field width too large"), "{d:?}");
        let (_, st, d) = pf(&["%.99999999999d|", "1"]);
        assert_eq!(st, 1);
        assert!(d[0].contains("precision too large"), "{d:?}");
    }

    #[test]
    fn floats() {
        assert_eq!(
            out(&["%.3f %e %g %G", "3.14159", "1234.5", "0.0001234", "1e20"]),
            "3.142 1.234500e+03 0.0001234 1E+20"
        );
        assert_eq!(out(&["%#x %#o %#g", "255", "8", "1.5"]), "0xff 010 1.50000");
        assert_eq!(out(&["%.0f %.0f %.0f", "0.5", "1.5", "2.5"]), "0 2 2");
        assert_eq!(
            out(&["%g %g %g %g", "100000", "1000000", "0.00001", "123456789"]),
            "100000 1e+06 1e-05 1.23457e+08"
        );
        assert_eq!(
            out(&["%f|%08.2f|%-8.2f|%+.1f", "1", "-3.14159", "2.5", "2"]),
            "1.000000|-0003.14|2.50    |+2.0"
        );
        assert_eq!(
            out(&["%e|%E|%.0e|%#.0e", "0", "123", "5", "5"]),
            "0.000000e+00|1.230000E+02|5e+00|5.e+00"
        );
        assert_eq!(
            out(&["%g|%g|%.3g|%g", "0", "1", "3.14159", "100"]),
            "0|1|3.14|100"
        );
        assert_eq!(
            out(&["%f %f %F %05f", "inf", "-inf", "nan", "inf"]),
            "inf -inf NAN   inf"
        );
        assert_eq!(
            out(&["%a|%A|%.2a", "1", "0.5", "1"]),
            "0x1p+0|0X1P-1|0x1.00p+0"
        );
        assert_eq!(out(&["%a|%a", "0", "255"]), "0x0p+0|0x1.fep+7");
        assert_eq!(
            out(&["%.1a|%.1a|%.1a", "1.96875", "1.09375", "1.15625"]),
            "0x2.0p+0|0x1.2p+0|0x1.2p+0"
        );
        assert_eq!(
            out(&["%.0a|%.0a|%.0a", "1.96875", "1.5", "2.5"]),
            "0x2p+0|0x2p+0|0x1p+1"
        );
        assert_eq!(
            out(&["%a", "4.9406564584124654e-324"]),
            "0x0.0000000000001p-1022"
        );
        assert_eq!(out(&["%f", "1e400"]), "inf");
        let (o, st, _) = pf(&["%f", "1.5x"]);
        assert_eq!((o.as_str(), st), ("1.500000", 1));
        assert_eq!(out(&["%.2f", "'A"]), "65.00");
        // C99 hexadecimal floats are accepted like strtod.
        assert_eq!(
            out(&["%f|%g|%.1f|%a", "0x1.8p+1", "-0x10", "0xAp-2", "0x.8"]),
            "3.000000|-16|2.5|0x1p-1"
        );
        let (o, st, _) = pf(&["%f", "0x1.8z"]);
        assert_eq!((o.as_str(), st), ("1.500000", 1));
        let (o, st, _) = pf(&["%f", "0x"]);
        assert_eq!((o.as_str(), st), ("0.000000", 1));
    }

    #[test]
    fn percent_b() {
        assert_eq!(out(&["%b\\n", "a\\0101\\tb"]), "aA\tb\n");
        assert_eq!(out(&["%b\\n", "a\\101"]), "aA\n");
        assert_eq!(out(&["%b|END", "x\\cy"]), "x");
        assert_eq!(out(&["%b\\n", "a\\0"]), "a\0\n");
        assert_eq!(
            out(&["%5b|%-5b|%.2b|", "ab", "ab", "abc"]),
            "   ab|ab   |ab|"
        );
        // \c stops even format reuse
        assert_eq!(out(&["%b", "a\\c", "b"]), "a");
    }

    #[test]
    fn errors_and_usage() {
        let (o, st, d) = pf(&[]);
        assert_eq!((o.as_str(), st), ("", 2));
        assert_eq!(d, vec!["usage: printf format [argument...]"]);
        let (o, st, d) = pf(&["abc%"]);
        assert_eq!((o.as_str(), st), ("abc", 1));
        assert_eq!(d, vec!["missing format character"]);
        let (o, st, d) = pf(&["%z\\n", "1"]);
        assert_eq!((o.as_str(), st), ("", 1));
        assert_eq!(d, vec!["%z: invalid directive"]);
        let (_, _, d) = pf(&["%5%|\\n"]);
        assert_eq!(d, vec!["%5%: invalid directive"]);
    }

    #[test]
    fn double_dash() {
        assert_eq!(out(&["--", "-x\\n"]), "-x\n");
        assert_eq!(out(&["%s\\n", "--"]), "--\n");
    }

    #[test]
    fn raw_bytes_round_trip() {
        // A byteenc-escaped 0xff byte in an argument comes out as 0xff.
        let arg = format!("a{}b", byteenc::escape_char(0xff));
        let args = vec!["%s|%c|%.2s".to_string(), arg.clone(), arg.clone(), arg];
        let r = run(&args);
        assert_eq!(r.out, b"a\xffb|a|a\xff".to_vec());
        // Octal escapes in the format produce raw bytes too.
        let r = run(&["\\377\\n".to_string()]);
        assert_eq!(r.out, b"\xff\n".to_vec());
    }
}
