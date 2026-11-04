//! Reading more lines of an incomplete command from non-seekable
//! standard input: prompting for them in an interactive shell, running
//! traps for signals that arrive while waiting, and batching the lines
//! that are already available.

use super::PendingInput;
use super::incomplete::{aliases_cannot_close, awaits_closer, completes_with_line, line_may_close};
use super::stdin_reader::{SignalCheck, StdinReader};
use crate::byteenc;
use crate::env;
use crate::exec::Executor;

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

/// Read more input for the pending command `input.text` into `raw`: a
/// line, waiting for it, then the further lines already available that
/// it is worth reading before the text is parsed again. `at_command_start`
/// says the text holds no command yet (the prompt is PS1, not PS2), and
/// `parser_open` that the parser ran out of text inside it without a
/// lexer error. Sets `input.retry_len` for the next attempt.
///
/// Returns whether an untrapped SIGINT in an interactive shell cancelled
/// the input (see `run_traps_while_waiting`).
pub(super) fn read_more_lines(
    executor: &mut Executor,
    reader: &mut StdinReader,
    input: &mut PendingInput,
    at_command_start: bool,
    parser_open: bool,
    raw: &mut Vec<u8>,
) -> bool {
    // An interactive shell (`sh -i` on a non-terminal)
    // prompts for each line: PS1 at a command start, PS2
    // for a continuation line (dash and bash agree).
    let mut ps1 = at_command_start;
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
        && input.pending_aliases.is_none()
        && aliases_cannot_close(input.aliases(executor))
        && awaits_closer(&input.text, || {
            completes_with_line(&input.text, input.aliases(executor), input.line)
        });
    prompt(executor);
    let mut interrupted = false;
    reader.fill(raw, 0, &mut || {
        run_traps_while_waiting(executor, &mut interrupted)
    });
    closer_free = closer_free && !line_may_close(raw);
    if !reader.eof && executor.exit_requested.is_none() && !interrupted {
        let mut checked = raw.len();
        while (closer_free || input.text.len() + raw.len() < input.retry_len)
            && reader.fill_ready_line(raw, &mut || {
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
    let len = input.text.len() + raw.len();
    input.retry_len = if len > REPARSE_EVERY_LINE_LIMIT {
        len + len / 8
    } else {
        0
    };
    interrupted
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
