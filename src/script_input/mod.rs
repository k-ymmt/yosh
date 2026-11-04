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

mod incomplete;
mod stdin_reader;

use crate::byteenc;
use crate::env;
use crate::exec::Executor;
use crate::parser::Parser;

use incomplete::{
    aliases_cannot_close, awaits_closer, completes_with_line, is_incomplete, is_lexer_incomplete,
    line_may_close,
};
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
