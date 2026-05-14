//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::parser::word::is_valid_name;

pub fn builtin_read(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    eprintln!("yosh: read: not implemented");
    Ok(1)
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
            None => return Ok(LineReadResult { bytes, hit_eof: true }),
            Some(b'\n') => return Ok(LineReadResult { bytes, hit_eof: false }),
            Some(b'\\') if !raw => {
                // Enter escape state.
                match reader.read_byte()? {
                    None => return Ok(LineReadResult { bytes, hit_eof: true }),
                    Some(b'\n') => continue, // line continuation: drop both
                    Some(other) => bytes.push(LineByte { value: other, escaped: true }),
                }
            }
            Some(other) => bytes.push(LineByte { value: other, escaped: false }),
        }
    }
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
            Ok(ParsedArgs { raw: false, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_dash_r_sets_raw() {
        assert_eq!(
            parse_args(&s(&["-r", "line"])),
            Ok(ParsedArgs { raw: true, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_double_dash_terminates_options() {
        // After `--`, even `-r` is treated as a (invalid) variable name.
        assert_eq!(
            parse_args(&s(&["--", "line"])),
            Ok(ParsedArgs { raw: false, var_names: vec!["line".into()] })
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
        assert_eq!(parse_args(&s(&["-x", "line"])), Err(ArgError::UnknownFlag('x')));
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
                bytes: vec![lb(b'h', false), lb(b'e', false), lb(b'l', false), lb(b'l', false), lb(b'o', false)],
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
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'b', false)],
        );
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
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'\\', false)],
        );
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
}
