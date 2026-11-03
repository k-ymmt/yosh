//! Process-level primitives for the shell and its forked children:
//! forking with the stdio locks held, signal masking around forks, and
//! fork-safe exits.

use nix::sys::signal::{SigSet, SigmaskHow, sigprocmask};

/// Fork the shell process with the std `stdout` / `stderr` locks held.
///
/// Rust's `std::io::stdout()` / `stderr()` handles are guarded by a
/// reentrant mutex plus a `LineWriter` buffer. A plain `fork()` copies
/// both into the child, so if ANY other thread holds one of those locks
/// at the fork instant, the child inherits it permanently locked and the
/// first `print!` / `flush()` in the child (notably [`exit_child`])
/// deadlocks. The shell parent is single-threaded in normal operation,
/// but the plugin host's exec-timeout watchdog, spec completion, and —
/// most often — the parallel `cargo test` harness (libtest's main thread
/// prints `test ... ok` lines) do run concurrent threads.
///
/// Holding both locks across the fork guarantees the forking thread is
/// the sole owner in the child, where the guards are released before
/// returning; the parent releases them likewise. Flushing before the
/// fork also prevents buffered output from being emitted twice (once by
/// each process). This is the `pthread_atfork` prepare/parent/child
/// pattern applied to the two std handles.
///
/// Lock order is stdout then stderr; nothing else in the codebase takes
/// both, so no ordering cycle is possible.
///
/// # Safety
///
/// Same contract as [`nix::unistd::fork`]: the child may only call
/// async-signal-safe functions until it `exec`s or `_exit`s. yosh runs
/// the in-process interpreter in the child regardless (see TODO.md,
/// Code Quality), so this wrapper narrows — not removes — the hazard.
pub(crate) unsafe fn fork_shell() -> nix::Result<nix::unistd::ForkResult> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let _ = out.flush();
    let _ = err.flush();
    // SAFETY: forwarded to the caller (see the doc comment).
    let result = unsafe { nix::unistd::fork() };
    // Release in reverse acquisition order in BOTH processes. In the
    // child the guards were acquired by the thread that forked, which is
    // the child's sole thread, so unlocking is well-defined.
    drop(err);
    drop(out);
    result
}

/// Exit a post-fork child process safely.
///
/// Uses `libc::_exit` to skip Rust runtime cleanup, which can deadlock
/// on std-internal mutexes inherited locked from a multithreaded parent
/// (e.g. `std::sys::pal::unix::stack_overflow::thread_info::LOCK`).
/// Flushes stdout/stderr first so buffered output is not lost.
///
/// Use ONLY after `fork()` in the child branch, never in the shell parent.
pub(crate) fn exit_child(status: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(status) }
}

/// Block every signal, returning the previous mask for
/// [`restore_signal_mask`]. Held across a fork whose child inherits the
/// parent's self-pipe handler AND the shared pipe until its own signal
/// setup runs: a signal delivered in that window would be written into
/// the pipe and later misread by the parent as its own.
pub(crate) fn block_all_signals() -> SigSet {
    let mut prev_mask = SigSet::empty();
    let _ = sigprocmask(
        SigmaskHow::SIG_BLOCK,
        Some(&SigSet::all()),
        Some(&mut prev_mask),
    );
    prev_mask
}

/// Restore a signal mask saved by [`block_all_signals`].
pub(crate) fn restore_signal_mask(mask: &SigSet) {
    let _ = sigprocmask(SigmaskHow::SIG_SETMASK, Some(mask), None);
}

/// Point stdin at /dev/null (an async list without job control, POSIX
/// §2.9.3.1). Left unchanged if /dev/null cannot be opened.
pub(crate) fn stdin_from_dev_null() {
    if let Ok(devnull) = std::fs::File::open("/dev/null") {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::dup2(devnull.as_raw_fd(), libc::STDIN_FILENO);
        }
    }
}

/// True once this process is a post-fork child of the shell (subshell,
/// pipeline member, command substitution, or async list). Set in every
/// `ForkResult::Child` branch so that exit paths reached *during* child
/// execution (e.g. the `exit` special built-in) can pick the fork-safe
/// exit.
static IN_FORKED_CHILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Mark this process as a post-fork child. Call first thing in every
/// `ForkResult::Child` branch.
pub(crate) fn mark_forked_child() {
    IN_FORKED_CHILD.store(true, std::sync::atomic::Ordering::Relaxed);
    super::redirect::forget_inherited_saved_redirections();
}

/// Exit the shell process, using [`exit_child`] when this process is a
/// post-fork child: `std::process::exit` runs Rust runtime cleanup that
/// can deadlock on std-internal mutexes inherited locked from a
/// multithreaded parent.
pub(crate) fn shell_exit(status: i32) -> ! {
    if IN_FORKED_CHILD.load(std::sync::atomic::Ordering::Relaxed) {
        exit_child(status);
    }
    std::process::exit(status);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fork children via `fork_fn` while a hog thread keeps re-acquiring
    /// the std stdout/stderr locks; each child must reach `_exit` through
    /// `exit_child` (which flushes both handles). Returns the number of
    /// children that had to be SIGKILLed after `deadline`.
    fn fork_under_stdio_lock_contention(
        fork_fn: unsafe fn() -> nix::Result<nix::unistd::ForkResult>,
        rounds: usize,
        deadline: std::time::Duration,
    ) -> usize {
        use nix::sys::signal::{Signal, kill};
        use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
        use nix::unistd::ForkResult;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let stop = Arc::new(AtomicBool::new(false));
        let hog = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    // Same acquisition order as fork_shell (stdout, stderr)
                    // so the hog never introduces a lock-order cycle; it
                    // simply keeps both handles locked almost all the time.
                    let _out = std::io::stdout().lock();
                    let _err = std::io::stderr().lock();
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
            })
        };
        // Let the hog reach its loop before the first fork.
        std::thread::sleep(std::time::Duration::from_millis(5));

        let mut killed = 0;
        for _ in 0..rounds {
            match unsafe { fork_fn() } {
                Err(e) => panic!("fork failed: {e}"),
                Ok(ForkResult::Child) => {
                    mark_forked_child();
                    exit_child(0);
                }
                Ok(ForkResult::Parent { child }) => {
                    let start = std::time::Instant::now();
                    loop {
                        match waitpid(child, Some(WaitPidFlag::WNOHANG)) {
                            Ok(WaitStatus::StillAlive) => {
                                if start.elapsed() > deadline {
                                    let _ = kill(child, Signal::SIGKILL);
                                    let _ = waitpid(child, None);
                                    killed += 1;
                                    break;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                            // Reaped here, or stolen by a concurrent test's
                            // `waitpid(-1)` (ECHILD): either way it exited.
                            Ok(_) | Err(nix::errno::Errno::ECHILD) => break,
                            Err(e) => panic!("waitpid failed: {e}"),
                        }
                    }
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        hog.join().unwrap();
        killed
    }

    /// Regression test for the command-substitution fork deadlock
    /// (TODO.md Code Quality, escalated 2026-09-02): a child forked while
    /// another thread held the std stdout lock inherited it locked and
    /// hung forever in `exit_child`'s `stdout().flush()`. `fork_shell`
    /// holds both stdio locks across the fork so the child always owns
    /// them. With a plain `nix::unistd::fork` this harness kills nearly
    /// every child (verified 2026-09-11: 20/20 killed).
    #[test]
    fn fork_shell_child_exits_despite_stdio_lock_contention() {
        let killed =
            fork_under_stdio_lock_contention(fork_shell, 20, std::time::Duration::from_secs(5));
        assert_eq!(killed, 0, "forked children deadlocked in exit_child");
    }
}
