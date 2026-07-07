//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::parser::word::is_valid_name;

pub fn builtin_read(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(ArgError::NoVarName) => {
            eprintln!("yosh: read: missing variable name");
            return Ok(1);
        }
        Err(ArgError::UnknownFlag(c)) => {
            eprintln!("yosh: read: -{}: invalid option", c);
            return Ok(1);
        }
        Err(ArgError::InvalidIdentifier(name)) => {
            eprintln!("yosh: read: `{}': not a valid identifier", name);
            return Ok(1);
        }
    };

    let mut reader = StdinByteReader;
    let result = match read_logical_line(parsed.raw, &mut reader) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("yosh: read: {}", e);
            return Ok(1);
        }
    };

    let ifs = match env.vars.get("IFS") {
        Some(s) => s.to_string(),
        None => " \t\n".to_string(),
    };
    let values = split_fields(&ifs, &result.bytes, parsed.var_names.len());

    for (name, value) in parsed.var_names.iter().zip(values) {
        if env.assign_var(name, value).is_err() {
            eprintln!("yosh: read: `{}': readonly variable", name);
            return Ok(1);
        }
    }

    if result.hit_eof { Ok(1) } else { Ok(0) }
}

/// Production `ByteReader` reading 1 byte at a time from fd 0,
/// retrying on EINTR.
struct StdinByteReader;

impl ByteReader for StdinByteReader {
    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        let mut buf = [0u8; 1];
        loop {
            // SAFETY: buf is a valid 1-byte stack allocation that lives for
            // the duration of this call; STDIN_FILENO is always open at
            // process start; the length matches the buffer size exactly.
            let n =
                unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr() as *mut libc::c_void, 1) };
            if n == 1 {
                return Ok(Some(buf[0]));
            }
            if n == 0 {
                return Ok(None);
            }
            // n == -1: check errno
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }
    }
}

#[derive(Debug, PartialEq)]
struct ParsedArgs {
    raw: bool,
    var_names: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum ArgError {
    NoVarName,
    UnknownFlag(char),
    InvalidIdentifier(String),
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, ArgError> {
    let mut raw = false;
    let mut idx = 0;
    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'r' => raw = true,
                other => return Err(ArgError::UnknownFlag(other)),
            }
        }
        idx += 1;
    }

    let var_names: Vec<String> = args[idx..].to_vec();
    if var_names.is_empty() {
        return Err(ArgError::NoVarName);
    }
    for name in &var_names {
        if !is_valid_name(name) {
            return Err(ArgError::InvalidIdentifier(name.clone()));
        }
    }
    Ok(ParsedArgs { raw, var_names })
}

/// A single byte of the logical line, plus a flag for whether it came
/// through a backslash escape (so split_and_assign can ignore IFS
/// classification for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineByte {
    value: u8,
    escaped: bool,
}

#[derive(Debug, PartialEq)]
struct LineReadResult {
    bytes: Vec<LineByte>,
    /// `true` if input ended before a newline was seen (partial line or
    /// no input at all). The caller assigns the available bytes and
    /// returns exit 1.
    hit_eof: bool,
}

/// Byte-by-byte stdin reader, abstracted so unit tests can inject
/// in-memory input without touching fd 0.
trait ByteReader {
    /// Returns `Ok(Some(b))` for a byte, `Ok(None)` for EOF, or
    /// `Err(io::Error)` for genuine read failures. Implementors must
    /// retry on `EINTR` internally.
    fn read_byte(&mut self) -> std::io::Result<Option<u8>>;
}

#[cfg(test)]
struct SliceReader<'a> {
    src: &'a [u8],
    pos: usize,
}

#[cfg(test)]
impl<'a> SliceReader<'a> {
    fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }
}

#[cfg(test)]
impl<'a> ByteReader for SliceReader<'a> {
    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        if self.pos >= self.src.len() {
            Ok(None)
        } else {
            let b = self.src[self.pos];
            self.pos += 1;
            Ok(Some(b))
        }
    }
}

fn read_logical_line<R: ByteReader>(raw: bool, reader: &mut R) -> std::io::Result<LineReadResult> {
    let mut bytes: Vec<LineByte> = Vec::new();
    loop {
        match reader.read_byte()? {
            None => {
                return Ok(LineReadResult {
                    bytes,
                    hit_eof: true,
                });
            }
            Some(b'\n') => {
                return Ok(LineReadResult {
                    bytes,
                    hit_eof: false,
                });
            }
            Some(b'\\') if !raw => {
                // Enter escape state.
                match reader.read_byte()? {
                    None => {
                        return Ok(LineReadResult {
                            bytes,
                            hit_eof: true,
                        });
                    }
                    Some(b'\n') => continue, // line continuation: drop both
                    Some(other) => bytes.push(LineByte {
                        value: other,
                        escaped: true,
                    }),
                }
            }
            Some(other) => bytes.push(LineByte {
                value: other,
                escaped: false,
            }),
        }
    }
}

/// POSIX §2.6.5 field splitting for `read`. Returns exactly `n_vars`
/// strings: var[0..n-1] are split fields, var[n-1] is the trailing
/// remainder (with only trailing whitespace-IFS trimmed). When IFS is
/// empty, no splitting occurs.
fn split_fields(ifs: &str, line: &[LineByte], n_vars: usize) -> Vec<String> {
    // The only caller (builtin_read) guarantees at least one var name
    // via ArgError::NoVarName.
    debug_assert!(n_vars >= 1);

    // Classify IFS bytes.
    let mut ws_ifs: Vec<u8> = Vec::new();
    let mut sep_ifs: Vec<u8> = Vec::new();
    for b in ifs.bytes() {
        if b == b' ' || b == b'\t' || b == b'\n' {
            ws_ifs.push(b);
        } else {
            sep_ifs.push(b);
        }
    }

    // Helper: is byte `b` an unescaped IFS byte of the given class?
    let is_ws = |lb: &LineByte| !lb.escaped && ws_ifs.contains(&lb.value);
    let is_sep = |lb: &LineByte| !lb.escaped && sep_ifs.contains(&lb.value);
    let is_any_ifs = |lb: &LineByte| is_ws(lb) || is_sep(lb);

    // Empty IFS → no splitting at all.
    if ws_ifs.is_empty() && sep_ifs.is_empty() {
        let whole: String = line.iter().map(|b| b.value as char).collect();
        let mut out = vec![whole];
        out.extend((1..n_vars).map(|_| String::new()));
        return out;
    }

    // Trim leading ws_ifs.
    let mut i = 0;
    while i < line.len() && is_ws(&line[i]) {
        i += 1;
    }

    // N=1 shortcut: trim trailing ws_ifs and return the whole remainder.
    if n_vars == 1 {
        let mut j = line.len();
        while j > i && is_ws(&line[j - 1]) {
            j -= 1;
        }
        let s: String = line[i..j].iter().map(|b| b.value as char).collect();
        return vec![s];
    }

    let mut result: Vec<String> = Vec::with_capacity(n_vars);

    // Emit fields 0..n_vars-2 (each terminated by IFS).
    for _ in 0..(n_vars - 1) {
        if i >= line.len() {
            result.push(String::new());
            continue;
        }
        // Field bytes: until the next IFS or end-of-line.
        let start = i;
        while i < line.len() && !is_any_ifs(&line[i]) {
            i += 1;
        }
        let field: String = line[start..i].iter().map(|b| b.value as char).collect();
        result.push(field);

        // Consume one terminator: at most one sep_ifs byte, then any
        // adjacent run of ws_ifs (which collapses with the terminator).
        if i < line.len() {
            if is_sep(&line[i]) {
                i += 1;
            }
            while i < line.len() && is_ws(&line[i]) {
                i += 1;
            }
        }
    }

    // Remainder for var[n_vars-1]: trim trailing ws_ifs only.
    let mut j = line.len();
    while j > i && is_ws(&line[j - 1]) {
        j -= 1;
    }
    let remainder: String = line[i..j].iter().map(|b| b.value as char).collect();
    result.push(remainder);

    debug_assert_eq!(result.len(), n_vars);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_args_no_args_is_error() {
        assert_eq!(parse_args(&[]), Err(ArgError::NoVarName));
    }

    #[test]
    fn parse_args_single_var() {
        assert_eq!(
            parse_args(&s(&["line"])),
            Ok(ParsedArgs {
                raw: false,
                var_names: vec!["line".into()]
            })
        );
    }

    #[test]
    fn parse_args_dash_r_sets_raw() {
        assert_eq!(
            parse_args(&s(&["-r", "line"])),
            Ok(ParsedArgs {
                raw: true,
                var_names: vec!["line".into()]
            })
        );
    }

    #[test]
    fn parse_args_double_dash_terminates_options() {
        // After `--`, even `-r` is treated as a (invalid) variable name.
        assert_eq!(
            parse_args(&s(&["--", "line"])),
            Ok(ParsedArgs {
                raw: false,
                var_names: vec!["line".into()]
            })
        );
    }

    #[test]
    fn parse_args_double_dash_then_dash_r_treats_as_invalid_ident() {
        // `--` terminates options; subsequent `-r` is a name and fails validation.
        assert_eq!(
            parse_args(&s(&["--", "-r"])),
            Err(ArgError::InvalidIdentifier("-r".into()))
        );
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        assert_eq!(
            parse_args(&s(&["-x", "line"])),
            Err(ArgError::UnknownFlag('x'))
        );
    }

    #[test]
    fn parse_args_invalid_identifier_errors() {
        assert_eq!(
            parse_args(&s(&["1foo"])),
            Err(ArgError::InvalidIdentifier("1foo".into()))
        );
    }

    #[test]
    fn parse_args_multiple_vars() {
        assert_eq!(
            parse_args(&s(&["-r", "x", "y", "z"])),
            Ok(ParsedArgs {
                raw: true,
                var_names: vec!["x".into(), "y".into(), "z".into()],
            })
        );
    }

    fn lb(value: u8, escaped: bool) -> LineByte {
        LineByte { value, escaped }
    }

    #[test]
    fn read_line_basic_terminates_at_newline() {
        let mut r = SliceReader::new(b"hello\nworld\n");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res,
            LineReadResult {
                bytes: vec![
                    lb(b'h', false),
                    lb(b'e', false),
                    lb(b'l', false),
                    lb(b'l', false),
                    lb(b'o', false)
                ],
                hit_eof: false,
            }
        );
    }

    #[test]
    fn read_line_partial_line_signals_eof() {
        let mut r = SliceReader::new(b"partial");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res.bytes.iter().map(|b| b.value).collect::<Vec<_>>(),
            b"partial".to_vec()
        );
        assert!(res.hit_eof);
    }

    #[test]
    fn read_line_eof_with_no_bytes() {
        let mut r = SliceReader::new(b"");
        let res = read_logical_line(false, &mut r).unwrap();
        assert!(res.bytes.is_empty());
        assert!(res.hit_eof);
    }

    #[test]
    fn read_line_backslash_newline_continues() {
        let mut r = SliceReader::new(b"a\\\nb\n");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(res.bytes, vec![lb(b'a', false), lb(b'b', false)],);
        assert!(!res.hit_eof);
    }

    #[test]
    fn read_line_backslash_other_keeps_literal_as_escaped() {
        let mut r = SliceReader::new(b"a\\bc\n");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'b', true), lb(b'c', false)],
        );
    }

    #[test]
    fn read_line_r_preserves_backslash_as_literal_byte() {
        let mut r = SliceReader::new(b"a\\b\n");
        let res = read_logical_line(true, &mut r).unwrap();
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'\\', false), lb(b'b', false)],
        );
    }

    #[test]
    fn read_line_r_backslash_newline_is_terminator() {
        // In -r mode, `\<newline>` is not line-continuation: the `\` is a
        // literal byte and the newline still ends the logical line.
        let mut r = SliceReader::new(b"a\\\nrest\n");
        let res = read_logical_line(true, &mut r).unwrap();
        assert_eq!(res.bytes, vec![lb(b'a', false), lb(b'\\', false)],);
        assert!(!res.hit_eof);
    }

    #[test]
    fn read_line_trailing_backslash_at_eof_in_nonraw_mode() {
        // Non-raw mode, input ends mid-escape ("a\" then EOF). Treat the
        // dangling backslash as if it were just dropped; bytes contains
        // only `a`, hit_eof=true.
        let mut r = SliceReader::new(b"a\\");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(res.bytes, vec![lb(b'a', false)]);
        assert!(res.hit_eof);
    }

    /// Reader implementing the ByteReader contract correctly: retries EINTR
    /// internally rather than propagating it to the caller.
    struct EintrRetryingReader<'a> {
        eintr_count: usize,
        inner: SliceReader<'a>,
    }

    impl<'a> ByteReader for EintrRetryingReader<'a> {
        fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
            loop {
                if self.eintr_count > 0 {
                    self.eintr_count -= 1;
                    continue; // simulate EINTR retry
                }
                return self.inner.read_byte();
            }
        }
    }

    #[test]
    fn read_line_eintr_retries() {
        // A ByteReader that retries EINTR internally (the contract for all
        // implementors). read_logical_line must transparently get all bytes.
        let mut r = EintrRetryingReader {
            eintr_count: 5,
            inner: SliceReader::new(b"hi\n"),
        };
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(res.bytes, vec![lb(b'h', false), lb(b'i', false)],);
        assert!(!res.hit_eof);
    }

    fn split_for(ifs: &str, line: Vec<LineByte>, n_vars: usize) -> Vec<String> {
        split_fields(ifs, &line, n_vars)
    }

    fn to_line(s: &str) -> Vec<LineByte> {
        s.bytes().map(|b| lb(b, false)).collect()
    }

    #[test]
    fn split_n_eq_1_trims_both_sides() {
        let out = split_for(" \t\n", to_line("   hello   "), 1);
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn split_n_eq_1_empty_input_yields_empty_string() {
        let out = split_for(" \t\n", to_line(""), 1);
        assert_eq!(out, vec!["".to_string()]);
    }

    #[test]
    fn split_n_gt_1_first_fields_then_remainder() {
        let out = split_for(" \t\n", to_line("a b c"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn split_remainder_keeps_internal_ifs() {
        let out = split_for(" \t\n", to_line("a b c d"), 2);
        assert_eq!(out, vec!["a".to_string(), "b c d".to_string()]);
    }

    #[test]
    fn split_leading_ifs_is_stripped() {
        let out = split_for(" \t\n", to_line("   a b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_trailing_ws_ifs_stripped_from_remainder() {
        let out = split_for(" \t\n", to_line("a b c   "), 2);
        assert_eq!(out, vec!["a".to_string(), "b c".to_string()]);
    }

    #[test]
    fn split_more_vars_than_fields_yields_empty_strings() {
        let out = split_for(" \t\n", to_line("a"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "".to_string()]);
    }

    #[test]
    fn split_empty_ifs_no_split() {
        let out = split_for("", to_line("a b c"), 2);
        // No splitting at all → entire line in var[0], var[1] empty.
        assert_eq!(out, vec!["a b c".to_string(), "".to_string()]);
    }

    #[test]
    fn split_sep_ifs_treated_as_single_separator() {
        // `IFS=:`, input "a::b" → x=a, y="" (one colon separator), z=b
        let out = split_for(":", to_line("a::b"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_mixed_sep_and_ws_ifs() {
        // IFS=":\t ", input "a: b" → ":" consumes one separator, then
        // " " is also IFS and is consumed greedily as adjacent ws.
        let out = split_for(": \t", to_line("a: b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_escaped_byte_not_treated_as_ifs() {
        // Non-raw "a\ b": split would normally see " " as IFS, but the
        // space is escaped (came from `\<space>`), so it stays in field 1.
        let line = vec![
            lb(b'a', false),
            lb(b' ', true), // escaped space
            lb(b'b', false),
        ];
        let out = split_fields(" \t\n", &line, 2);
        assert_eq!(out, vec!["a b".to_string(), "".to_string()]);
    }
}
