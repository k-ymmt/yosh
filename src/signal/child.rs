//! Signal setup for forked children: disposition reset, the
//! `posix_spawn` default-signal set, and the job-control overrides for
//! foreground / background children.

use std::collections::HashSet;

use super::disposition::{
    current_disposition, default_signal, disposition_may_have_changed, ignore_signal,
};
use super::entry::ignored_on_entry_set_opt;
use super::self_pipe::{close_self_pipe, reinit_self_pipe_after_fork, signal_handler};
use super::table::catchable_signals;

/// Whether a child keeps `num` at SIG_IGN: the user trap-ignored it
/// (`ignored`) or it was inherited as SIG_IGN at shell entry (§2.12,
/// `entry_set`).
fn keeps_ignored(num: i32, ignored: &[i32], entry_set: Option<&HashSet<i32>>) -> bool {
    ignored.contains(&num) || entry_set.is_some_and(|s| s.contains(&num))
}

/// Disposition walk shared by [`reset_child_signals`] (fork+exec
/// children) and [`reset_shell_child_signals`] (fork children that keep
/// running shell code): `ignored` signals (user `trap '' SIG`) retain
/// SIG_IGN, signals inherited as SIG_IGN at shell entry (§2.12) stay
/// ignored, everything else — including the parent's `HANDLED_SIGNALS`
/// self-pipe handlers and any `trap 'cmd' SIG` handlers — resets to
/// SIG_DFL.
///
/// Walks the full SIGNAL_TABLE (not just HANDLED_SIGNALS): the trap
/// builtin can install handlers/SIG_IGN for any trappable signal (e.g.
/// `trap 'cmd' ABRT`, `trap '' PIPE`), and the shell itself ignores the
/// job-control signals in monitor mode — all of those must come back to
/// SIG_DFL in children unless the user trap-ignored them. Callers that
/// need job-control exceptions (`setup_*_child_signals`) adjust after.
///
/// Signals the process never changed (see
/// [`disposition_may_have_changed`]) and the user did not trap-ignore
/// already have exactly that disposition and are skipped: a script's
/// children then make ~8 sigaction calls instead of ~30.
fn reset_child_dispositions(ignored: &[i32]) {
    let entry_set = ignored_on_entry_set_opt();
    for num in catchable_signals() {
        if !disposition_may_have_changed(num) && !ignored.contains(&num) {
            continue;
        }
        let keep_ignored = keeps_ignored(num, ignored, entry_set);
        if !keep_ignored
            && (num == libc::SIGSEGV || num == libc::SIGBUS)
            && has_foreign_handler(num)
        {
            // The Rust runtime's stack-overflow reporter lives on
            // SEGV/BUS. Only a disposition the shell installed (trap
            // handler / SIG_IGN) needs undoing; keep the runtime's so a
            // shell child still reports a stack overflow instead of a
            // bare SIGSEGV. (exec resets caught handlers anyway.)
            continue;
        }
        if keep_ignored {
            ignore_signal(num);
        } else {
            default_signal(num);
        }
    }
}

/// `true` when `sig` currently has a caught handler that is not the
/// shell's own self-pipe `signal_handler` (i.e. someone else — the
/// Rust runtime — installed it).
fn has_foreign_handler(sig: i32) -> bool {
    let Some(old) = current_disposition(sig) else {
        return false;
    };
    let h = old.sa_sigaction;
    h != libc::SIG_DFL
        && h != libc::SIG_IGN
        && h != signal_handler as *const () as libc::sighandler_t
}

/// Signals a `posix_spawn`ed external command must start with at
/// SIG_DFL (`POSIX_SPAWN_SETSIGDEF`) so its dispositions equal what
/// [`reset_child_signals`] followed by `execve` gives a forked child:
/// every `SIGNAL_TABLE` signal not kept ignored (caught handlers would
/// reset on exec anyway). Kept-ignored signals are inherited, so this
/// returns `None` — use the fork path — when one of them is not
/// currently SIG_IGN in the shell (spawn attributes cannot set SIG_IGN).
pub fn spawn_default_sigset(ignored: &[i32]) -> Option<libc::sigset_t> {
    let entry_set = ignored_on_entry_set_opt();
    // SAFETY: sigset_t is plain data, initialized by sigemptyset.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigemptyset(&mut set) };
    for num in catchable_signals() {
        if keeps_ignored(num, ignored, entry_set) {
            let is_ignored =
                current_disposition(num).is_some_and(|old| old.sa_sigaction == libc::SIG_IGN);
            if !is_ignored {
                return None;
            }
        } else {
            // SAFETY: `set` is initialized; `num` is a valid signal.
            unsafe { libc::sigaddset(&mut set, num) };
        }
    }
    Some(set)
}

/// Reset signals after fork for children that will `exec` (or exit
/// without running further shell code). See
/// [`reset_child_dispositions`] for the disposition policy; the
/// self-pipe fds are closed since an exec'd child has no use for them
/// (they are CLOEXEC anyway — closing here also covers exec failure).
pub fn reset_child_signals(ignored: &[i32]) {
    reset_child_dispositions(ignored);
    close_self_pipe();
}

/// Reset signals after fork for a child that CONTINUES running shell
/// code (command substitution, `(...)` subshell, pipeline member,
/// async `&` list). Two differences from the parent state matter:
///
/// * Dispositions: signals the parent kept on its self-pipe handler —
///   the always-registered `HANDLED_SIGNALS` and any `trap 'cmd' SIG`
///   — go to SIG_DFL. POSIX §2.12: traps caught by the parent are reset
///   to default in a subshell, so a USR1/TERM/… must kill the child
///   immediately, even mid-blocking-syscall, instead of being deferred
///   by the leftover SA_RESTART handler to the next command boundary
///   (where it only takes effect after e.g. a long `sleep` completes).
///   `trap '' SIG` and entry-ignored signals stay SIG_IGN.
///
/// * Self-pipe: the child must NOT share the parent's pipe — a byte
///   written by one process's handler could be drained by the other.
///   The inherited fds are replaced with a fresh pipe
///   (`reinit_self_pipe_after_fork`) so the child's OWN `trap`
///   invocations (which reinstall the self-pipe handler via
///   [`apply_trap_disposition`](super::apply_trap_disposition)) keep
///   working.
///
/// The forking parent itself is untouched — the interactive REPL and a
/// `yosh -c` top level keep their handlers and pipe.
pub fn reset_shell_child_signals(ignored: &[i32]) {
    reset_child_dispositions(ignored);
    reinit_self_pipe_after_fork();
}

/// Foreground job-control overrides applied on top of the base child
/// reset: SIGTSTP, SIGTTIN, SIGTTOU to SIG_DFL so the child can be
/// stopped (even when the parent inherited them SIG_IGN at entry),
/// unless the user trap-ignored them.
fn apply_foreground_child_overrides(ignored: &[i32]) {
    if !ignored.contains(&libc::SIGTSTP) {
        default_signal(libc::SIGTSTP);
    }
    if !ignored.contains(&libc::SIGTTIN) {
        default_signal(libc::SIGTTIN);
    }
    if !ignored.contains(&libc::SIGTTOU) {
        default_signal(libc::SIGTTOU);
    }
}

/// Background job-control overrides applied on top of the base child
/// reset: SIGTTIN ignored to prevent background reads from stopping the
/// job; SIGTSTP/SIGTTOU stoppable unless user trap-ignored.
fn apply_background_child_overrides(ignored: &[i32]) {
    ignore_signal(libc::SIGTTIN);
    if !ignored.contains(&libc::SIGTSTP) {
        default_signal(libc::SIGTSTP);
    }
    if !ignored.contains(&libc::SIGTTOU) {
        default_signal(libc::SIGTTOU);
    }
}

/// Set up signals for a foreground child process that will `exec`.
pub fn setup_foreground_child_signals(ignored: &[i32]) {
    reset_child_signals(ignored);
    apply_foreground_child_overrides(ignored);
}

/// Set up signals for a foreground fork child that continues running
/// shell code (monitor-mode pipeline member): same dispositions as
/// [`setup_foreground_child_signals`], but the self-pipe is re-created
/// instead of closed so traps set inside the child keep working (see
/// [`reset_shell_child_signals`]).
pub fn setup_foreground_shell_child_signals(ignored: &[i32]) {
    reset_shell_child_signals(ignored);
    apply_foreground_child_overrides(ignored);
}

/// Set up signals for a background fork child that continues running
/// shell code (monitor-mode async `&` list): base child reset with the
/// background job-control overrides, with the self-pipe re-created
/// instead of closed (see [`reset_shell_child_signals`]). Background
/// externals are exec'd from inside this child via the standard
/// foreground path, so no exec'ing background variant exists.
pub fn setup_background_shell_child_signals(ignored: &[i32]) {
    reset_shell_child_signals(ignored);
    apply_background_child_overrides(ignored);
}
