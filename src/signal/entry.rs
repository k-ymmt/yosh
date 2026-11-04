//! Signals inherited with SIG_IGN at shell entry (POSIX §2.12).

use std::collections::HashSet;
use std::sync::OnceLock;

use super::disposition::current_disposition;
use super::table::catchable_signals;

/// Signals inherited with SIG_IGN disposition at shell entry.
/// Per POSIX §2.12, these signals cannot be trapped or reset by the shell.
/// Captured once at startup before any yosh handler is installed; never mutated
/// afterward, so a stale `get()` from a fork/exec child reflects the correct
/// entry state (because the global is inherited as a copy of the parent's set).
static IGNORED_ON_ENTRY: OnceLock<HashSet<i32>> = OnceLock::new();

/// Query each trappable POSIX signal's current disposition via `sigaction(_, NULL, &mut old)`
/// and return the set of signals currently set to SIG_IGN.
/// Must be called before any yosh handler is installed to correctly observe
/// what was inherited from the parent process.
fn capture_ignored_on_entry() -> HashSet<i32> {
    let mut set = HashSet::new();
    for num in catchable_signals() {
        if num == libc::SIGPIPE {
            // The Rust std runtime sets SIGPIPE to SIG_IGN before main(),
            // so an observed SIG_IGN here is (almost always) the runtime's
            // doing, not an inherited disposition from the invoking
            // process. Treating it as ignored-on-entry made `trap ... PIPE`
            // a silent no-op, listed a phantom `trap -- '' SIGPIPE`, and
            // kept SIGPIPE ignored in every child (`find | head` broke).
            // The runtime's clobber makes the true inherited state
            // unrecoverable; assume the common case (not ignored).
            // init_signal_handling restores SIG_DFL right after this.
            continue;
        }
        let Some(old) = current_disposition(num) else {
            continue;
        };
        if old.sa_sigaction == libc::SIG_IGN {
            set.insert(num);
        }
    }
    set
}

/// Capture the ignored-on-entry set on first call (later calls return
/// the set captured then). Called by
/// [`init_signal_handling`](super::init_signal_handling) before it
/// installs any handler.
pub(super) fn capture_ignored_on_entry_once() -> &'static HashSet<i32> {
    IGNORED_ON_ENTRY.get_or_init(capture_ignored_on_entry)
}

/// Returns `true` if `sig` was inherited with SIG_IGN disposition at shell startup.
/// Returns `false` if [`init_signal_handling`](super::init_signal_handling)
/// has not been called yet.
pub fn is_ignored_on_entry(sig: i32) -> bool {
    IGNORED_ON_ENTRY.get().is_some_and(|set| set.contains(&sig))
}

/// Like [`ignored_on_entry_set`] but returns `None` if the capture has not
/// happened yet (useful for callers that must not panic, e.g. `TrapStore::listing`).
pub fn ignored_on_entry_set_opt() -> Option<&'static HashSet<i32>> {
    IGNORED_ON_ENTRY.get()
}

/// Returns a reference to the set of ignored-on-entry signals.
///
/// # Panics
///
/// Panics if [`init_signal_handling`](super::init_signal_handling) has
/// not been called.
#[allow(dead_code)]
pub fn ignored_on_entry_set() -> &'static HashSet<i32> {
    IGNORED_ON_ENTRY
        .get()
        .expect("init_signal_handling() must be called first")
}

#[cfg(test)]
mod tests {
    use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

    use super::super::init_signal_handling;
    use super::*;

    // -----------------------------------------------------------------------
    // Sub-project 5 — Task 1: Ignored-on-entry capture tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_ignored_on_entry_false_for_unlikely_signal() {
        // After init (possibly already called by other tests), a benign signal
        // that is extremely unlikely to be inherited as SIG_IGN in a `cargo test`
        // run should report `false`. SIGALRM is a safe choice — its number (14)
        // is identical on Linux and macOS AND is present in SIGNAL_TABLE, so
        // the assertion actually exercises the capture path on both platforms.
        init_signal_handling();
        assert!(
            !is_ignored_on_entry(libc::SIGALRM),
            "SIGALRM should not be ignored-on-entry in a normal test environment"
        );
    }

    #[test]
    fn test_capture_ignored_on_entry_detects_sig_ign() {
        // IMPORTANT: Initialize IGNORED_ON_ENTRY with a clean signal state
        // BEFORE we mutate SIGALRM. This ensures that parallel tests running
        // is_ignored_on_entry(...) or init_signal_handling() do not observe
        // this test's mid-flight SIG_IGN as part of the "inherited at entry"
        // set. OnceLock::get_or_init guarantees atomic one-shot init.
        init_signal_handling();

        // It exercises `capture_ignored_on_entry` directly to verify the
        // sigaction query logic. We use SIGALRM (14) which is in SIGNAL_TABLE
        // on both Linux (num 14) and macOS (num 14). On macOS, SIGUSR2=31
        // is not in SIGNAL_TABLE, so SIGALRM is used instead. We restore the
        // original disposition afterward to avoid polluting sibling tests.
        let sig_num = libc::SIGALRM;

        // Save the current disposition.
        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(sig_num, std::ptr::null(), &mut original) };
        assert_eq!(rc, 0);

        // Install SIG_IGN.
        let ign_sa = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
        let sig = Signal::try_from(sig_num).unwrap();
        unsafe {
            sigaction(sig, &ign_sa).unwrap();
        }

        // Run the capture helper and assert SIGALRM is in the set.
        let captured = capture_ignored_on_entry();
        assert!(
            captured.contains(&sig_num),
            "capture_ignored_on_entry should detect SIGALRM SIG_IGN, got {:?}",
            captured
        );

        // Restore original disposition.
        let rc = unsafe { libc::sigaction(sig_num, &original, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_capture_ignored_on_entry_excludes_default() {
        // IMPORTANT: Initialize IGNORED_ON_ENTRY with a clean signal state
        // BEFORE we mutate SIGPIPE. This ensures that parallel tests running
        // is_ignored_on_entry(...) or init_signal_handling() do not observe
        // this test's mid-flight SIG_DFL mutation as part of the captured set.
        // OnceLock::get_or_init guarantees atomic one-shot init.
        init_signal_handling();

        // SIGPIPE (13) at SIG_DFL should NOT appear in the captured set.
        // SIGPIPE is in SIGNAL_TABLE on both Linux and macOS with number 13.
        let sig_num = libc::SIGPIPE;

        let mut original: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(sig_num, std::ptr::null(), &mut original) };
        assert_eq!(rc, 0);

        let dfl_sa = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
        let sig = Signal::try_from(sig_num).unwrap();
        unsafe {
            sigaction(sig, &dfl_sa).unwrap();
        }

        let captured = capture_ignored_on_entry();
        assert!(
            !captured.contains(&sig_num),
            "capture_ignored_on_entry should not include SIG_DFL signals, got {:?}",
            captured
        );

        // Restore.
        let rc = unsafe { libc::sigaction(sig_num, &original, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }
}
