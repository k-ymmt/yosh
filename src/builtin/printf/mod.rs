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

mod float;
mod operand;
mod text;

use crate::builtin::options::strip_end_of_options;
use crate::builtin::output::write_stdout_bytes;
use crate::byteenc;
use crate::error::ShellError;
use float::{fmt_a, fmt_e, fmt_f, fmt_g};
use text::{char_len, digits, first_char_len, truncate_chars, unescape_into};

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
        && let Err(e) = write_stdout_bytes(&run.out)
    {
        run.diags.push(format!("write error: {}", e));
    }
    for d in &run.diags {
        crate::byteenc::write_stderr_decoded_line(&format!("yosh: printf: {}", d));
    }
    Ok(if run.usage {
        2
    } else if run.diags.is_empty() {
        0
    } else {
        1
    })
}

/// Evaluate `printf` over byteenc-encoded `args` without performing I/O.
fn run(args: &[String]) -> Run {
    let mut r = Run {
        out: Vec::new(),
        diags: Vec::new(),
        usage: false,
    };
    let args = strip_end_of_options(args);
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
        if fmt.get(start + 1) == Some(&b'%') {
            self.r.out.push(b'%');
            return Ok(Some(start + 2));
        }
        let (spec, mut i) = self.spec(fmt, start)?;
        let Some(&conv) = fmt.get(i) else {
            return Err("missing format character".to_string());
        };
        i += 1;
        match conv {
            b'd' | b'i' | b'u' | b'o' | b'x' | b'X' => self.integer(&spec, conv),
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => self.float(&spec, conv),
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
                if self.percent_b(&spec) {
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

    /// Parse the flags, width and precision of the directive starting at
    /// `fmt[start] == b'%'` (consuming `*` operands). Returns them and the
    /// index of the conversion character.
    fn spec(&mut self, fmt: &[u8], start: usize) -> Result<(Spec, usize), String> {
        let mut i = start + 1;
        let mut spec = Spec::default();
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
        Ok((spec, i))
    }

    /// `%d %i %u %o %x %X`.
    fn integer(&mut self, spec: &Spec, conv: u8) {
        let (neg, prefix, body) = match conv {
            b'd' | b'i' => {
                let v = self.signed_arg();
                (v < 0, "", v.unsigned_abs().to_string())
            }
            b'u' => (false, "", self.unsigned_arg().to_string()),
            b'o' => {
                let v = self.unsigned_arg();
                let mut body = format!("{:o}", v);
                if spec.alt && !body.starts_with('0') {
                    body.insert(0, '0');
                }
                (false, "", body)
            }
            _ => {
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
                (false, prefix, body)
            }
        };
        let kind = NumKind::Int {
            octal_alt: conv == b'o' && spec.alt,
            signed: matches!(conv, b'd' | b'i'),
        };
        self.emit_number(spec, neg, prefix, body, kind);
    }

    /// `%e %E %f %F %g %G %a %A`.
    fn float(&mut self, spec: &Spec, conv: u8) {
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
        self.emit_number(spec, neg, prefix, body, kind);
    }

    /// `%b`: the operand with its backslash escapes expanded. Returns
    /// true when a `\c` in it stops all further output.
    fn percent_b(&mut self, spec: &Spec) -> bool {
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
        self.emit_padded(spec, body);
        stop
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
