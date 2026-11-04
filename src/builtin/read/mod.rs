//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

mod split;
mod stdin;

use crate::builtin::options::{invalid_flag, scan_flag_words};
use crate::error::ShellError;
use crate::exec::Executor;
use crate::parser::word::is_valid_name;

use split::split_fields;
use stdin::{StdinByteReader, handle_signals_during_read};

pub fn builtin_read(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(ArgError::NoVarName) => {
            eprintln!("yosh: read: missing variable name");
            return Ok(1);
        }
        Err(ArgError::UnknownFlag(c)) => {
            // Usage error: status 2 like dash/bash (and yosh's other
            // builtins), distinct from read's EOF/assignment status 1.
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: {}",
                invalid_flag("read", c)
            ));
            return Ok(2);
        }
        Err(ArgError::InvalidIdentifier(name)) => {
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: read: `{}': not a valid identifier",
                name
            ));
            return Ok(1);
        }
    };

    let mut deferred_traps = Vec::new();
    let (line_result, aborted) = {
        let mut reader = StdinByteReader::new(|waiting| {
            handle_signals_during_read(executor, &mut deferred_traps, waiting)
        });
        let line_result = read_logical_line(parsed.raw, &mut reader);
        // Seek fd 0 back over any block-buffered bytes beyond the consumed
        // line, on success AND error paths, so the next reader of the fd
        // starts exactly where this `read` logically stopped.
        reader.rewind_unconsumed();
        (line_result, reader.aborted)
    };
    // POSIX §2.11: a trapped signal received while the shell waits for a
    // foreground utility runs its action once the utility has completed.
    // The trapped signals the read took in go back on the self-pipe, so
    // the executor's next drain point — after this command, with its
    // assignments made, its status in `$?` and its redirections undone —
    // runs their actions, as for any other utility (dash agrees: `trap
    // 'x=timeout' ALRM` overrides the partial line the read assigns).
    crate::signal::requeue_signals(&deferred_traps);
    // A signal abandoned the read (see `handle_signals_during_read`): no
    // variable is assigned and the status reports the signal. A trapped
    // signal that only *ended* the read looks like end-of-file instead:
    // what was read so far is assigned below and the status is 1.
    if let Some(status) = aborted {
        return Ok(status);
    }
    let env = &mut executor.env;
    let result = match line_result {
        Ok(r) => r,
        Err(e) => {
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: read: {}",
                crate::exec::redirect::io_error_text(&e)
            ));
            return Ok(1);
        }
    };

    let ifs = match env.vars.get("IFS") {
        Some(s) => s.to_string(),
        None => " \t\n".to_string(),
    };
    let values = split_fields(&ifs, &result.bytes, parsed.var_names.len());

    // Assignments are applied in order and NOT rolled back on failure:
    // if the Kth variable is readonly, variables 1..K-1 keep their new
    // values when the error fires. POSIX leaves this implementation-
    // defined; bash exhibits the same partial-assignment behavior.
    for (name, value) in parsed.var_names.iter().zip(values) {
        if env.assign_var(name, value).is_err() {
            crate::byteenc::write_stderr_decoded_line(&format!(
                "yosh: read: `{}': readonly variable",
                name
            ));
            return Ok(1);
        }
    }

    if result.hit_eof { Ok(1) } else { Ok(0) }
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
    let idx = scan_flag_words(args, |ch| match ch {
        'r' => {
            raw = true;
            true
        }
        _ => false,
    })
    .map_err(|e| ArgError::UnknownFlag(e.flag))?;

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
    /// `Err(io::Error)` for genuine read failures. Implementors handle
    /// `EINTR` internally (retrying, or ending the read with an error).
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
                    Some(0) => continue,
                    Some(other) => bytes.push(LineByte {
                        value: other,
                        escaped: true,
                    }),
                }
            }
            // A NUL byte cannot be part of a shell variable value; drop
            // it (dash and bash both do) rather than store it.
            Some(0) => continue,
            Some(other) => bytes.push(LineByte {
                value: other,
                escaped: false,
            }),
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

    #[test]
    fn read_line_drops_nul_bytes() {
        let mut r = SliceReader::new(b"a\0b\\\0c\n");
        let res = read_logical_line(false, &mut r).unwrap();
        let bytes: Vec<u8> = res.bytes.iter().map(|b| b.value).collect();
        assert_eq!(bytes, b"abc");
    }
}
