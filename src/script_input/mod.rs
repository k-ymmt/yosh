//! The top-level command loop for non-REPL input: a `-c` string, a
//! script file, or commands read from standard input.
//!
//! Standard input needs care (XCU sh, INPUT FILES): when the shell reads
//! its commands from stdin and runs a command that also reads stdin, that
//! command must see the input right after the command the shell read —
//! the shell may not read ahead into it. `printf 'read x\nhi\necho $x\n'
//! | sh` must print `hi`. So stdin is read incrementally:
//!
//! * a regular file is read in blocks, and before each command runs the
//!   file offset is moved back to the end of that command's text; if the
//!   command moved the offset (it consumed input), reading resumes there;
//! * anything else (pipe, FIFO, terminal, device) is read one byte at a
//!   time up to each newline, and more lines are read only while the
//!   commands read so far are incomplete.

mod stdin_reader;

use crate::byteenc;
use crate::env;
use crate::error::{ParseErrorKind, ShellErrorKind};
use crate::exec::Executor;
use crate::parser::Parser;

use stdin_reader::SignalCheck;
pub use stdin_reader::StdinReader;

/// Where the command text comes from.
pub enum Source<'a> {
    /// The whole input is known up front (`-c` string, script file).
    Text(&'a str),
    /// Standard input, read without reading ahead.
    Stdin(StdinReader),
}

/// Unconsumed command text larger than this is re-parsed only after it
/// grew by an eighth since the last incomplete attempt (non-seekable
/// input), keeping a long compound command linear to parse instead of
/// re-parsing it once per line. Below it every line is tried, so no
/// line after a complete command is read before that command runs —
/// except lines that cannot complete it (see `line_may_close`), which are
/// read without re-parsing at any length.
/// The batching reads only input that is already available: once the
/// input runs dry the text is re-parsed at any length, so a complete
/// command never waits for more input from a writer that keeps the
/// stream open.
const REPARSE_EVERY_LINE_LIMIT: usize = 16 * 1024;

/// Run every command from `source` and return the shell's exit status
/// (running the EXIT trap first).
pub fn run_commands(executor: &mut Executor, source: Source<'_>) -> i32 {
    match source {
        Source::Text(text) => run_text(executor, text, None),
        Source::Stdin(reader) => run_text(executor, "", Some(reader)),
    }
}

/// Whether a parse error means the text ended in the middle of a command
/// (so more input may complete it) rather than a real syntax error.
fn is_incomplete(p: &Parser, kind: &ShellErrorKind) -> bool {
    p.is_at_end()
        || matches!(
            kind,
            ShellErrorKind::Parse(
                ParseErrorKind::UnterminatedSingleQuote
                    | ParseErrorKind::UnterminatedDoubleQuote
                    | ParseErrorKind::UnterminatedCommandSub
                    | ParseErrorKind::UnterminatedArithSub
                    | ParseErrorKind::UnterminatedParamExpansion
                    | ParseErrorKind::UnterminatedBacktick
                    | ParseErrorKind::UnterminatedDollarSingleQuote
                    | ParseErrorKind::UnexpectedEof
                    | ParseErrorKind::InvalidHereDoc
            )
        )
}

/// Whether a parse error is the lexer's: the text ended inside a quote,
/// an expansion or a here-document body. More input then continues that
/// construct, whose end may be any line (a here-document delimiter).
fn is_lexer_incomplete(kind: &ShellErrorKind) -> bool {
    matches!(
        kind,
        ShellErrorKind::Parse(
            ParseErrorKind::UnterminatedSingleQuote
                | ParseErrorKind::UnterminatedDoubleQuote
                | ParseErrorKind::UnterminatedCommandSub
                | ParseErrorKind::UnterminatedArithSub
                | ParseErrorKind::UnterminatedParamExpansion
                | ParseErrorKind::UnterminatedBacktick
                | ParseErrorKind::UnterminatedDollarSingleQuote
                | ParseErrorKind::InvalidHereDoc
        )
    )
}

/// Bytes that end a word, so the next byte may start a token: the
/// lexer's delimiters, plus NUL (which separates commands in a script).
fn is_word_delimiter(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | 0
    )
}

/// Whether `line` (one line of input, read while the pending command is
/// still inside an unclosed compound command) might complete that
/// command. Errs towards true: a false answer must be certain.
///
/// Only a closing token completes a compound command: `}`, `)`, `fi`,
/// `done` or `esac` (no alias can supply one, see `aliases_cannot_close`).
/// The line is scanned as if it started at the lexer's top level — every
/// line passed here is, as long as the lines before it got false: those
/// hold no quotes or backslashes (which could continue a word onto the
/// next line), no here-document operators (whose body lines are
/// arbitrary), and leave no parenthesis or `${` open at their end.
/// Within such a line a `)` that closes a construct opened on an earlier
/// line drives the parenthesis depth below zero, and a reserved word
/// starts right after a delimiter.
fn line_may_close(line: &[u8]) -> bool {
    // `(`/`)` nesting outside `${...}`, and `${...}` nesting (bare
    // braces inside one counted too, so the span covers whatever the
    // lexer takes for the expansion).
    let (mut parens, mut braces) = (0i32, 0i32);
    let mut i = 0;
    while i < line.len() {
        let b = line[i];
        let token_start = i == 0 || is_word_delimiter(line[i - 1]);
        match b {
            b'\'' | b'"' | b'`' | b'\\' => return true,
            b'<' if line.get(i + 1) == Some(&b'<') => return true,
            b'$' if line.get(i + 1) == Some(&b'{') => {
                braces += 1;
                i += 2;
                continue;
            }
            b'{' if braces > 0 => braces += 1,
            b'}' if token_start => return true,
            b'}' if braces > 0 => braces -= 1,
            // A parenthesis inside `${...}` may be pattern text, which
            // the depth must not count.
            b'(' | b')' if braces > 0 => return true,
            b'(' => parens += 1,
            b')' => {
                parens -= 1;
                if parens < 0 {
                    return true;
                }
            }
            // A comment runs to the end of the line; inside an open
            // `$(` it would hide the `)`.
            b'#' if token_start => return parens != 0 || braces != 0,
            b'f' | b'd' | b'e' if token_start => {
                for word in [&b"fi"[..], b"done", b"esac"] {
                    if line[i..].starts_with(word)
                        && line
                            .get(i + word.len())
                            .is_none_or(|&c| is_word_delimiter(c))
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    parens != 0 || braces != 0
}

/// Whether no alias value can bring a closing token (or anything else
/// `line_may_close` looks for) into a line: substituting such values
/// keeps every verdict of `line_may_close` true to the line's tokens.
/// Nor may a value end in an operator or `)` (`alias a='true &&'`,
/// `alias d='f()'`), or be blank (`true && b` with `alias b=''`): a line
/// ending with such an alias may leave the command waiting for any next
/// line, not a closer, which `awaits_closer` judges from the raw text.
fn aliases_cannot_close(aliases: &env::aliases::AliasStore) -> bool {
    aliases.is_empty()
        || aliases.sorted_iter().iter().all(|(_, value)| {
            !value.contains('#')
                && !matches!(
                    value.trim_end_matches([' ', '\t', '\n']).as_bytes().last(),
                    None | Some(b'&' | b'|' | b'!' | b'<' | b'>' | b'(' | b')' | b'\\')
                )
                && value.split('\n').all(|l| !line_may_close(l.as_bytes()))
        })
}

/// Whether the pending command `text` — incomplete, the parser having
/// reached the end of it without a lexer error — can only be completed by
/// a closing token, i.e. it sits inside an unclosed compound command
/// rather than after an operator (`&&`, `||`, `|`, `!`) that any next
/// line could follow. `completes_with_line` parses the text with one more
/// simple command (`:`) appended and says whether that completes it; it
/// is used only when a cheap look at the last line cannot tell (a
/// comment there, or here-documents whose bodies follow the operator).
fn awaits_closer(text: &str, completes_with_line: impl FnOnce() -> bool) -> bool {
    let bytes = text.as_bytes();
    if !bytes.ends_with(b"\n") {
        return false;
    }
    if bytes.windows(2).any(|w| w == b"<<") {
        return !completes_with_line();
    }
    // The last line holding a token: after an operator only blank lines
    // and comments may follow.
    let Some(last) = bytes
        .split(|&b| b == b'\n')
        .rev()
        .map(|l| {
            let end = l.iter().rposition(|&b| b != b' ' && b != b'\t');
            &l[..end.map_or(0, |e| e + 1)]
        })
        .find(|l| !l.is_empty())
    else {
        return false;
    };
    let comment = last
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'#' && (i == 0 || is_word_delimiter(last[i - 1])));
    if comment {
        return !completes_with_line();
    }
    // After `f()` the body may be any compound command, `( )` included,
    // whose balanced parentheses `line_may_close` does not count.
    !matches!(
        last.last(),
        Some(b'\\' | b'&' | b'|' | b'!' | b'<' | b'>' | b')')
    )
}

/// Whether `text` followed by the line `:` parses as a complete command
/// (the probe of `awaits_closer`).
fn completes_with_line(text: &str, aliases: &env::aliases::AliasStore, line: usize) -> bool {
    let probe = format!("{text}:\n");
    let mut p = Parser::resuming_at_line(&probe, aliases, line, None);
    let parsed = p.skip_newlines().and_then(|()| {
        if p.is_at_end() {
            return Ok(false);
        }
        p.parse_complete_command().map(|_| true)
    });
    matches!(parsed, Ok(true)) && !p.is_at_end()
}

/// Newlines in the first `end` bytes of `text` (counted as bytes, so
/// `end` need not be a char boundary).
fn count_newlines(text: &str, end: usize) -> usize {
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// Run the trap actions for signals that arrived while the shell was
/// reading command input. Says to abandon the read when one of them
/// asked the shell to exit, or cancelled the input.
///
/// An untrapped SIGINT in an interactive shell cancels the command
/// being entered, like Ctrl+C at a terminal prompt: `interrupted` is
/// set, and the caller discards the input read for it (dash and bash
/// drop their buffered input).
///
/// An interactive shell (`sh -i` reading a non-terminal) records an
/// untrapped fatal signal (HUP, ALRM, USR1, …) as an exit request
/// instead of dying on the spot; blocked in a read, nothing else would
/// act on it before the next line arrives. The shell then dies of that
/// signal (its EXIT trap already ran; see `Executor::exit_signal`), as
/// dash and bash do.
fn run_traps_while_waiting(executor: &mut Executor, interrupted: &mut bool) -> SignalCheck {
    let signals = crate::signal::drain_pending_signals();
    if signals.is_empty() {
        return SignalCheck::Quiet;
    }
    let mut cancel = false;
    for sig in signals {
        let untrapped = matches!(
            executor.env.traps.get_signal_trap(sig),
            None | Some(env::traps::TrapAction::Default)
        );
        executor.run_signal_traps(&[sig]);
        if executor.exit_requested.is_some() {
            return SignalCheck::Abandon;
        }
        if untrapped && sig == libc::SIGINT && executor.env.mode.is_interactive {
            *interrupted = true;
            cancel = true;
        }
    }
    if cancel {
        SignalCheck::Abandon
    } else {
        SignalCheck::Handled
    }
}

/// Expand and print a prompt variable to stderr.
fn write_prompt(executor: &mut Executor, var: &str) {
    use std::io::Write;
    let prompt = crate::interactive::prompt::expand_prompt(&mut executor.env, var);
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(&byteenc::decode_bytes(&prompt));
    let _ = stderr.flush();
}

fn run_text(executor: &mut Executor, initial: &str, mut reader: Option<StdinReader>) -> i32 {
    if executor.env.mode.is_interactive
        && let Some(r) = reader.as_mut()
    {
        // Line-at-a-time reading, so each line gets its prompt.
        r.seekable = false;
    }
    // The unexecuted input, starting at a command boundary (encoded).
    let mut text = initial.to_string();
    // Line number of text[0], for diagnostics and LINENO.
    let mut line = 1usize;
    // Seekable stdin: file offset of text[0].
    let mut text_raw_start = reader.as_ref().map_or(0, |r| r.read_pos);
    // Non-seekable stdin: a long incomplete command is retried only once
    // the text reaches this length.
    let mut retry_len = 0usize;
    // Alias replacement tokens still to be read at text[0]: a command
    // from an alias whose value spans several lines ran, and the rest of
    // the value (`alias a='echo 1\nfalse &&'`) continues the input. A
    // parser rebuilt over the remaining text resumes them.
    let mut pending_aliases = None;
    // Non-seekable stdin: the alias table the command at text[0] was
    // first parsed with, kept while more lines are read to complete it.
    // A trap run meanwhile may redefine an alias (`trap "alias a=..."
    // USR1`), but words already read keep the substitution they got
    // when read (dash and bash substitute aliases as a word is lexed);
    // re-parsing the command's text with the new table would change
    // them. The rest of the command sees the old table too, as in dash
    // and bash, which run the trap only once the command is complete.
    let mut cmd_aliases: Option<env::aliases::AliasStore> = None;
    let mut status = 0;

    'rebuild: loop {
        let at_eof = reader.as_ref().is_none_or(|r| r.eof);
        // Parse and execute one complete command at a time so that
        // aliases defined by earlier commands are available for later
        // ones.
        //
        // One parser reads the text. `parse_complete_command` stops at
        // the newline ending a command without lexing the next line, so
        // the next line is tokenized only after this command ran: aliases
        // it defines apply (the alias table is refreshed before each
        // command), a lexer error on a later line cannot discard it, and
        // tokens queued by an alias whose value spans several lines are
        // kept.
        let mut p = Parser::resuming_at_line(
            &text,
            cmd_aliases.as_ref().unwrap_or(&executor.env.aliases),
            line,
            pending_aliases.clone(),
        );
        // Once the whole input is here, end of input ends an
        // unterminated here-document, as in dash and bash.
        p.set_eof_ends_heredoc(at_eof);
        let mut echoed = 0;
        // Start of the command being parsed (end of the previous one).
        let mut cmd_start = 0usize;
        // Raw-byte bookkeeping for seekable stdin: encoded position
        // `enc_mark` is raw offset `raw_mark` from text_raw_start.
        let (mut enc_mark, mut raw_mark) = (0usize, 0usize);
        // Seekable stdin: the fd offset the last command ran with and the
        // end of its text, until the fd's offset now is compared with it
        // (whether the command read input or replaced fd 0).
        let mut unchecked: Option<(i64, usize)> = None;

        loop {
            p.set_aliases(cmd_aliases.as_ref().unwrap_or(&executor.env.aliases));
            let parsed = p.skip_newlines().and_then(|()| {
                if p.is_at_end() {
                    return Ok(None);
                }
                p.parse_complete_command().map(Some)
            });
            // More input may change what was parsed: the text ran out
            // before a newline ended the command (a trailing `\` or
            // operator, an open compound command, quote or here-document).
            let needs_more = !at_eof
                && match &parsed {
                    Ok(None) => true,
                    Ok(Some(_)) => p.is_at_end(),
                    Err(e) => is_incomplete(&p, &e.kind),
                };
            // Check the fd offset after the last command (parsing is
            // pure, so the next command may be parsed first from text
            // that turns out stale). When that command is ready to run,
            // one relative lseek both checks the offset and moves it to
            // the end of the next command; otherwise check it now.
            let mut positioned = None;
            if let Some((off, end)) = unchecked.take() {
                let r = reader.as_mut().expect("only seekable stdin defers a check");
                let moved = if matches!(parsed, Ok(Some(_))) && !needs_more {
                    let next_end = p.input_position();
                    raw_mark += byteenc::decode_bytes(&text[enc_mark..next_end]).len();
                    enc_mark = next_end;
                    let next = text_raw_start + raw_mark as i64;
                    positioned = Some(next);
                    !r.seek_from(off, next)
                } else {
                    r.offset() != Some(off)
                };
                if moved {
                    // The command read from (or replaced) stdin: continue
                    // reading where it left off.
                    line += count_newlines(&text, end);
                    text.clear();
                    r.detect();
                    text_raw_start = r.read_pos;
                    continue 'rebuild;
                }
            }
            if needs_more {
                // The parser ran out of text inside the command, without
                // a lexer error (see `awaits_closer`).
                let parser_open =
                    matches!(&parsed, Err(e) if p.is_at_end() && !is_lexer_incomplete(&e.kind));
                let reader = reader.as_mut().expect("needs_more implies a reader");
                // (Traps run only while non-seekable input is read.)
                if cmd_aliases.is_none() && !reader.seekable && !matches!(parsed, Ok(None)) {
                    cmd_aliases = Some(executor.env.aliases.clone());
                }
                // Drop the executed commands and read more after the rest.
                line += count_newlines(&text, cmd_start);
                if reader.seekable {
                    text_raw_start += raw_mark as i64
                        + byteenc::decode_bytes(&text[enc_mark..cmd_start]).len() as i64;
                }
                text.drain(..cmd_start);
                let mut raw = Vec::new();
                if reader.seekable {
                    reader.fill(&mut raw, text.len(), &mut || SignalCheck::Quiet);
                } else {
                    // An interactive shell (`sh -i` on a non-terminal)
                    // prompts for each line: PS1 at a command start, PS2
                    // for a continuation line (dash and bash agree).
                    let mut ps1 = matches!(parsed, Ok(None));
                    let interactive = executor.env.mode.is_interactive;
                    let mut prompt = |executor: &mut Executor| {
                        if interactive {
                            if ps1 {
                                // Job status changes are reported before
                                // the PS1 prompt, as in the REPL.
                                executor.reap_zombies();
                                executor.display_job_notifications();
                            }
                            write_prompt(executor, if ps1 { "PS1" } else { "PS2" });
                            ps1 = false;
                        }
                    };
                    // Read a line, then more while the pending text is
                    // still short of the re-parse length — but only whole
                    // lines already available: the shell must never block
                    // for input while what it holds may be a complete
                    // command (a writer that keeps the pipe open waits
                    // for its output). Once the input runs dry the text
                    // is re-parsed whatever its length.
                    //
                    // At any length, while the command is inside an
                    // unclosed compound command, available lines that
                    // cannot close it are read without re-parsing it for
                    // each (re-parsing the whole text per line made a
                    // long `{ }` group or function quadratic). Not in an
                    // interactive shell, which reports a syntax error in
                    // such a line before prompting for the next.
                    let mut closer_free = parser_open
                        && !interactive
                        && pending_aliases.is_none()
                        && aliases_cannot_close(
                            cmd_aliases.as_ref().unwrap_or(&executor.env.aliases),
                        )
                        && awaits_closer(&text, || {
                            completes_with_line(
                                &text,
                                cmd_aliases.as_ref().unwrap_or(&executor.env.aliases),
                                line,
                            )
                        });
                    prompt(executor);
                    let mut interrupted = false;
                    reader.fill(&mut raw, 0, &mut || {
                        run_traps_while_waiting(executor, &mut interrupted)
                    });
                    closer_free = closer_free && !line_may_close(&raw);
                    if !reader.eof && executor.exit_requested.is_none() && !interrupted {
                        let mut checked = raw.len();
                        while (closer_free || text.len() + raw.len() < retry_len)
                            && reader.fill_ready_line(&mut raw, &mut || {
                                run_traps_while_waiting(executor, &mut interrupted)
                            })
                        {
                            // (The prompt for a line read without waiting
                            // follows it; there was no wait to prompt for.)
                            prompt(executor);
                            closer_free = closer_free && !line_may_close(&raw[checked..]);
                            checked = raw.len();
                        }
                    }
                    let len = text.len() + raw.len();
                    retry_len = if len > REPARSE_EVERY_LINE_LIMIT {
                        len + len / 8
                    } else {
                        0
                    };
                    if interrupted && executor.exit_requested.is_none() {
                        // SIGINT cancelled the command being entered:
                        // drop everything read for it, as Ctrl+C does at
                        // a terminal (dash and bash), and prompt anew on
                        // a fresh line with `$?` 130 (dash).
                        line += count_newlines(&text, text.len())
                            + raw.iter().filter(|&&b| b == b'\n').count();
                        text.clear();
                        reader.discard_partial_line();
                        pending_aliases = None;
                        cmd_aliases = None;
                        retry_len = 0;
                        status = 128 + libc::SIGINT;
                        executor.env.exec.last_exit_status = status;
                        byteenc::write_stderr_decoded_line("");
                        continue 'rebuild;
                    }
                }
                // A signal trap (or an untrapped signal in an interactive
                // shell) asked the shell to exit while it waited for input.
                if let Some(code) = executor.exit_requested {
                    status = code;
                    break 'rebuild;
                }
                text.push_str(&byteenc::encode_bytes(&raw));
                continue 'rebuild;
            }
            retry_len = 0;
            // set -v: echo input to stderr as it is read (POSIX §2.15 set).
            executor.echo_read_input(&p, &mut echoed);
            match parsed {
                Ok(None) => break 'rebuild,
                Ok(Some(cmd)) => {
                    let end = p.input_position();
                    // Seekable stdin: leave the fd offset right after this
                    // command's text while it runs.
                    let expected = match reader.as_mut() {
                        Some(_) if positioned.is_some() => positioned,
                        Some(r) if r.seekable => {
                            raw_mark += byteenc::decode_bytes(&text[enc_mark..end]).len();
                            enc_mark = end;
                            let off = text_raw_start + raw_mark as i64;
                            r.seek(off);
                            Some(off)
                        }
                        _ => None,
                    };
                    cmd_start = end;
                    pending_aliases = p.pending_aliases();
                    cmd_aliases = None;
                    // `sh -c`: with only blank text left, this command is
                    // the shell's last; its final simple command may
                    // replace the shell (Executor::mark_shell_tail).
                    // Not under `set -v`, which still echoes that text.
                    if reader.is_none()
                        && executor.env.mode.options.cmd_string
                        && !executor.env.mode.options.verbose
                        && pending_aliases.is_none()
                        && text[end..]
                            .bytes()
                            .all(|b| matches!(b, b' ' | b'\t' | b'\n'))
                    {
                        executor.mark_shell_tail();
                    }
                    status = executor.exec_complete_command(&cmd);
                    // Check for flow control (exit handled by std::process::exit in builtin)
                    match executor.env.exec.flow_control {
                        // A fatal expansion error (nounset, ${x:?}, …) exits a
                        // non-interactive shell; an interactive shell aborts
                        // the current command line only and keeps reading.
                        Some(env::FlowControl::ExpansionError)
                            if executor.env.mode.is_interactive =>
                        {
                            executor.env.exec.flow_control = None;
                        }
                        // The command that raised it may have reported status 0
                        // (`for i in ${x:?}; do :; done`); the shell still exits
                        // with the expansion-error status (POSIX §2.8.1: non-zero).
                        Some(env::FlowControl::ExpansionError) => {
                            status = 1;
                            executor.env.exec.last_exit_status = status;
                            break 'rebuild;
                        }
                        Some(_) => break 'rebuild,
                        None => {}
                    }
                    // POSIX §2.8.1 shell errors request exit via exit_requested.
                    if let Some(code) = executor.exit_requested {
                        status = code;
                        break 'rebuild;
                    }
                    executor.check_errexit(status);
                    // Whether the command moved the fd offset is checked
                    // once the next command has been parsed.
                    unchecked = expected.map(|off| (off, end));
                    // The command replaced stdin after its end had been
                    // seen (`exec <file` on an unterminated last line):
                    // read on from the new file after the rest of the
                    // text, as dash does.
                    if let Some(r) = reader.as_mut()
                        && r.input_replaced()
                    {
                        line += count_newlines(&text, end);
                        text.drain(..end);
                        r.reopen();
                        continue 'rebuild;
                    }
                }
                Err(e) if executor.env.mode.is_interactive => {
                    // POSIX §2.8.1: a syntax error does not end an
                    // interactive shell — discard the rest of the line
                    // it is on and read on, with `$?` set to 2.
                    byteenc::write_stderr_decoded_line(&e.to_string());
                    status = 2;
                    executor.env.exec.last_exit_status = status;
                    // Searched as bytes: `from` steps back one byte from
                    // the parser's position, which may land inside a
                    // multi-byte character (`{ :; } é`); a newline byte
                    // never does, so `end` is always a char boundary.
                    let from = p.input_position().saturating_sub(1).max(cmd_start);
                    let end = text.as_bytes()[from.min(text.len())..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(text.len(), |i| from + i + 1);
                    line += count_newlines(&text, end);
                    // (Interactive stdin is never read in seekable block
                    // mode, so there is no file offset to adjust.)
                    text.drain(..end);
                    pending_aliases = None;
                    cmd_aliases = None;
                    continue 'rebuild;
                }
                Err(e) => {
                    byteenc::write_stderr_decoded_line(&e.to_string());
                    // The EXIT trap sees the status the shell exits with
                    // (dash: `trap 'echo $?' EXIT` then a syntax error
                    // prints 2).
                    executor.env.exec.last_exit_status = 2;
                    executor.process_pending_signals();
                    executor.execute_exit_trap();
                    return 2;
                }
            }
        }
    }

    executor.process_pending_signals();
    executor.execute_exit_trap();
    if let Some(sig) = executor.exit_signal {
        crate::exec::shell_exit_by_signal(sig);
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_may_close_flags_every_possible_closing_token() {
        for line in [
            "}\n",
            "  :; }\n",
            "a;}\n",
            ")\n",
            "echo a) b\n",
            "( a ) )\n",
            "fi\n",
            "x; fi;\n",
            "done\n",
            "done>f\n",
            "  esac\n",
            "\0fi\n",
            // Quotes, escapes and here-documents may continue a word or
            // a body onto the next line.
            "echo 'a\n",
            "echo \"a\n",
            "echo `a\n",
            "echo a\\\n",
            "echo \\}\n",
            "cat <<E\n",
            "x=$((1<<2))\n",
            // Openers left open at the end of the line.
            "x=$(echo a\n",
            "x=${y\n",
            "( a\n",
            "echo $(( 1 + (2\n",
            // Parentheses inside `${...}`, and a brace that may end it
            // or be a reserved word.
            "echo ${x#(} )\n",
            "echo ${x:-{} }\n",
            "echo ${x:- }\n",
            // A comment inside an open `$(`.
            "x=$(echo # )\n",
        ] {
            assert!(line_may_close(line.as_bytes()), "{line:?}");
        }
    }

    #[test]
    fn line_may_close_passes_lines_without_closing_tokens() {
        for line in [
            ":\n",
            "x=$((x + 1))\n",
            "echo ${x} ${y:-z} ${#x} $# ${x#*.}\n",
            "y=$(date) z=$( (echo) )\n",
            "if true; then\n",
            "for i in a b; do\n",
            "f() {\n",
            "while :; do {\n",
            "file=final fix=1 done_x=1 esacs=1 x=fi\n",
            "echo a}b {}\n",
            "# a comment with } ) fi 'quotes' and <<\n",
            "echo a # done ) }\n",
            "case $x in\n",
            "\n",
        ] {
            assert!(!line_may_close(line.as_bytes()), "{line:?}");
        }
    }

    #[test]
    fn aliases_that_could_close_disable_the_shortcut() {
        let mut aliases = env::aliases::AliasStore::default();
        assert!(aliases_cannot_close(&aliases));
        aliases.set("ll", "ls -l");
        assert!(aliases_cannot_close(&aliases));
        for value in [
            "}",
            "fi",
            "echo )",
            "x #",
            "echo 'a",
            "a\nb; }",
            "true &&",
            "x | ",
            "!",
            "f()",
            "",
            "  ",
            "ls $(pwd)",
        ] {
            let mut a = aliases.clone();
            a.set("bad", value);
            assert!(!aliases_cannot_close(&a), "{value:?}");
        }
    }

    /// `awaits_closer` with the real probe, for text that the parser
    /// reports as incomplete without a lexer error.
    fn awaits(text: &str) -> bool {
        let aliases = env::aliases::AliasStore::default();
        let mut p = Parser::resuming_at_line(text, &aliases, 1, None);
        let parsed = p.skip_newlines().and_then(|()| p.parse_complete_command());
        match parsed {
            Err(e) => assert!(
                p.is_at_end() && !is_lexer_incomplete(&e.kind),
                "{text:?}: {e}"
            ),
            Ok(_) => panic!("{text:?} parsed as complete"),
        }
        awaits_closer(text, || completes_with_line(text, &aliases, 1))
    }

    #[test]
    fn awaits_closer_inside_compound_commands() {
        for text in [
            "{\n",
            "{\n: a\n",
            "f() {\n",
            "if true\n",
            "if true; then\n  :\n",
            "while :; do\n",
            "for i in a b\n",
            "case x in\n",
            "case x in\n a) :;;\n",
            "(\n echo a\n",
            "{\n:\n# a comment\n",
            "{\n: a # b\n",
            "{\ncat <<E\nbody\nE\n",
            "{\n{ cat <<E; }\nbody\nE\n",
        ] {
            assert!(awaits(text), "{text:?}");
        }
    }

    #[test]
    fn any_line_may_complete_after_an_operator() {
        for text in [
            "true &&\n",
            "true ||\n\n",
            "true |\n",
            "{\n:\n} &&\n",
            "true && # comment\n",
            "true &&\n# comment\n",
            "true &&\n  # comment\n\n",
            "cat <<E &&\nbody\nE\n",
            "cat <<E |\nbody\nE\n",
            "true &&\\\n",
            // The body after `f()` may be a balanced `( )`.
            "f()\n",
        ] {
            assert!(!awaits(text), "{text:?}");
        }
    }

    #[test]
    fn here_document_bodies_and_quotes_are_lexer_incomplete() {
        // `awaits_closer` relies on these never reaching it: their next
        // line may be any line.
        let aliases = env::aliases::AliasStore::default();
        for text in [
            "{\ncat <<E\n",
            "{\ncat <<E\nbody\n",
            "{\ncat <<-E\n\tbody\n",
            "{\ncat <<E; cat <<F\nbody\nE\n",
            "{\necho 'a\n",
            "{\necho \"a\n",
            "{\nx=$(\n",
            "{\nx=${y\n",
            "{\necho `a\n",
        ] {
            let mut p = Parser::resuming_at_line(text, &aliases, 1, None);
            let err = p
                .skip_newlines()
                .and_then(|()| p.parse_complete_command())
                .expect_err(text);
            assert!(is_lexer_incomplete(&err.kind), "{text:?}: {err}");
        }
    }
}
