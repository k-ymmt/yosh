//! Reading the logical line from fd 0 for `read`: the fd-kind
//! classification, block or byte reads, waiting for input, and the
//! handling of signals that arrive meanwhile.

use super::ByteReader;
use crate::env::{FlowControl, TrapAction};
use crate::exec::Executor;

/// Block size for buffered reads from seekable input. One page is plenty:
/// the goal is to amortize the read(2)-per-byte syscall cost (~40
/// syscalls/line before), not to stream large volumes.
const READ_BLOCK: usize = 4096;

/// What `handle_signals_during_read` decided about the read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignalOutcome {
    /// No signal was pending: nothing ran.
    NoSignal,
    /// Signals arrived and the read goes on. Input readiness is checked
    /// again before the next read(2), in case a signal action touched fd
    /// 0 (cheap, and keeps the reader from blocking on stale readiness).
    Handled,
    /// A trapped signal ended the read like end-of-file: the bytes read
    /// so far are assigned and the status is 1.
    EndRead,
    /// The read is abandoned with this status and nothing is assigned.
    Abort(i32),
}

/// Handle the signals that arrived during `read` and decide whether the
/// read goes on. `waiting` tells whether the read is blocked on idle
/// input or only between read(2) calls that have data ready.
///
/// Signals with a trap action are only collected in `deferred`: as in
/// dash, their actions run once the read has completed (see
/// `builtin_read`), so an action sees — and may override — what the read
/// assigned. A trapped signal that arrives while the read waits ends the
/// read as end-of-file would: the partial line read so far is assigned
/// (with the usual splitting) and the status is 1, so `trap ... ALRM` can
/// time out a `read`. With data ready the read goes on (dash keeps
/// reading too), and so it does for a trapped SIGCHLD, which a finished
/// background job raises at any time.
///
/// The default action of an untrapped signal runs right here: a fatal
/// one ends a non-interactive shell (inside `run_signal_traps`) instead
/// of only once a line completes — which never happens for endless input
/// without a newline (`read x </dev/zero`). An interactive shell's
/// untrapped SIGINT abandons the read (130) either way, and with it the
/// rest of the command line — like Ctrl+C on a foreground job in dash and
/// bash (`read x; echo WRONG` prints nothing).
pub(super) fn handle_signals_during_read(
    executor: &mut Executor,
    deferred: &mut Vec<i32>,
    waiting: bool,
) -> SignalOutcome {
    // No syscall unless a handler has fired since the last drain: this
    // runs before every read(2) of a busy read.
    let signals = crate::signal::drain_pending_signals();
    if signals.is_empty() {
        return SignalOutcome::NoSignal;
    }
    let traps = &executor.env.traps;
    let (trapped, untrapped): (Vec<i32>, Vec<i32>) = signals
        .iter()
        .copied()
        .partition(|&sig| matches!(traps.get_signal_trap(sig), Some(TrapAction::Command(_))));
    let ends_read = waiting && trapped.iter().any(|&sig| sig != libc::SIGCHLD);
    let interactive_int = executor.env.mode.is_interactive
        && untrapped.contains(&libc::SIGINT)
        && !matches!(
            traps.get_signal_trap(libc::SIGINT),
            Some(TrapAction::Ignore)
        );
    for sig in trapped {
        if !deferred.contains(&sig) {
            deferred.push(sig);
        }
    }
    executor.run_signal_traps(&untrapped);
    if interactive_int {
        // Unwind to the prompt the way a fatal expansion error does in an
        // interactive shell: every list, compound command and function
        // stops (`should_unwind`), and the REPL / `-c` loop clears the
        // flag before the next command line.
        if executor.env.exec.flow_control.is_none() {
            executor.env.exec.flow_control = Some(FlowControl::ExpansionError);
        }
        return SignalOutcome::Abort(128 + libc::SIGINT);
    }
    if executor.should_unwind() {
        let last = untrapped.last().copied();
        return SignalOutcome::Abort(128 + last.unwrap_or(signals[signals.len() - 1]));
    }
    if ends_read {
        return SignalOutcome::EndRead;
    }
    SignalOutcome::Handled
}

/// Whether the last `read` found fd 0 to be a regular file: a hint for
/// which system call `classify_stdin` tries first.
static LAST_STDIN_REGULAR: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `(regular, seekable)` for fd 0.
///
/// A successful lseek means the fd has a file offset we can rewind;
/// ESPIPE (pipe/FIFO/socket) and failures in general mean it does not —
/// use the conservative per-byte path. A regular file also never blocks
/// a read(2), so it needs no readiness check (a terminal, also seekable
/// on macOS, does). One system call settles the common cases: fstat for
/// a regular file, lseek for a pipe; which one goes first follows the
/// previous call, as a loop reads the same input over and over (fstat
/// costs about 1.5x an lseek, and a readiness check as much again).
fn classify_stdin() -> (bool, bool) {
    use std::sync::atomic::Ordering::Relaxed;
    let file_type = || {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstat fills `st` for a valid fd and fails otherwise.
        (unsafe { libc::fstat(libc::STDIN_FILENO, &mut st) } == 0)
            .then_some(st.st_mode & libc::S_IFMT)
    };
    let has_offset = || unsafe { libc::lseek(libc::STDIN_FILENO, 0, libc::SEEK_CUR) } != -1;
    let (regular, seekable) = if LAST_STDIN_REGULAR.load(Relaxed) {
        match file_type() {
            Some(libc::S_IFREG) => (true, true),
            Some(libc::S_IFIFO | libc::S_IFSOCK) => (false, false),
            _ => (false, has_offset()),
        }
    } else if has_offset() {
        (file_type() == Some(libc::S_IFREG), true)
    } else {
        (false, false)
    };
    LAST_STDIN_REGULAR.store(regular, Relaxed);
    (regular, seekable)
}

/// Production `ByteReader` for fd 0.
///
/// POSIX requires `read` not to consume input past the logical line, so
/// the safe universal strategy is one read(2) per byte. That costs ~40
/// syscalls per typical line — measured 6x slower than bash on a
/// 20k-line `while read` loop. When fd 0 is *seekable* (a regular file:
/// `lseek(fd, 0, SEEK_CUR)` succeeds), we can batch: read in
/// `READ_BLOCK` chunks and, once the line is complete, lseek back over
/// whatever the buffer holds beyond the consumed newline
/// (`rewind_unconsumed`). Pipes, FIFOs, and ttys — where lseek fails
/// with ESPIPE — keep the byte-at-a-time reads, preserving the
/// must-not-over-consume semantics exactly.
///
/// Byte semantics are unchanged either way: this only batches syscalls;
/// every byte still flows through `read_byte` untouched (byteenc
/// escaping happens later, in `field_to_string`).
///
/// Signals: most of the shell's handlers use `SA_RESTART`, so a read(2)
/// blocked on an idle pipe or terminal would sleep straight through a
/// SIGINT or SIGALRM (and HUP/TERM's EINTR used to be retried as well).
/// Before any read(2) that could block — FIONREAD reports nothing
/// buffered — the reader instead polls fd 0 together with the signal
/// self-pipe and hands arriving signals to `on_signal(true)`. Input that
/// is always readable (`/dev/zero`, a fast writer) never blocks, so
/// every read(2) with data ready is preceded by `on_signal(false)` — an
/// atomic flag test when no signal is pending. Once signals were
/// handled, readiness is checked again before the next read(2): one that
/// blocks would stop dispatching later signals until input arrives.
pub(super) struct StdinByteReader<F: FnMut(bool) -> SignalOutcome> {
    /// Whether fd 0 is a regular file, which read(2) never blocks on:
    /// no readiness check is needed (checked once at construction).
    regular: bool,
    /// Whether fd 0 is seekable (checked once at construction).
    seekable: bool,
    /// Buffered bytes from the last block read (seekable mode only).
    buf: Vec<u8>,
    /// Next unconsumed index into `buf`.
    pos: usize,
    /// Bytes FIONREAD reported readable that have not been read yet:
    /// read(2) cannot block while this is non-zero.
    ready: usize,
    /// Handles pending signals (the argument: whether the read is
    /// waiting for input) and says whether the read goes on.
    on_signal: F,
    /// `on_signal` returned `EndRead`: the reader reports end-of-file.
    ended: bool,
    /// The status of an `Abort` returned by `on_signal`.
    pub(super) aborted: Option<i32>,
}

impl<F: FnMut(bool) -> SignalOutcome> StdinByteReader<F> {
    pub(super) fn new(on_signal: F) -> Self {
        let (regular, seekable) = classify_stdin();
        StdinByteReader {
            regular,
            seekable,
            buf: Vec::new(),
            pos: 0,
            ready: 0,
            on_signal,
            ended: false,
            aborted: None,
        }
    }

    /// Seek fd 0 back over the buffered-but-unconsumed bytes so the file
    /// offset lands exactly after the last byte this reader handed out
    /// (i.e. just past the consumed newline, or at EOF). No-op for
    /// non-seekable input and when the buffer is fully drained.
    pub(super) fn rewind_unconsumed(&mut self) {
        if !self.seekable {
            return;
        }
        let unread = self.buf.len() - self.pos;
        if unread > 0 {
            // Best-effort: lseek succeeded at construction on this same
            // fd, so a failure here is not actionable — the offset then
            // stays past the line, which is the pre-batching worst case
            // for an fd that lied about seekability.
            unsafe { libc::lseek(libc::STDIN_FILENO, -(unread as libc::off_t), libc::SEEK_CUR) };
        }
        self.buf.clear();
        self.pos = 0;
    }

    /// Let `on_signal` handle pending signals. Returns whether any signal
    /// action ran; an end or abort of the read becomes an `Interrupted`
    /// error that unwinds to `read_byte`, which reports end-of-file for an
    /// end and passes the error on for an abort.
    fn signal_arrived(&mut self, waiting: bool) -> std::io::Result<bool> {
        match (self.on_signal)(waiting) {
            SignalOutcome::NoSignal => Ok(false),
            SignalOutcome::Handled => Ok(true),
            SignalOutcome::EndRead => {
                self.ended = true;
                Err(std::io::ErrorKind::Interrupted.into())
            }
            SignalOutcome::Abort(status) => {
                self.aborted = Some(status);
                Err(std::io::ErrorKind::Interrupted.into())
            }
        }
    }

    /// Wait until a read(2) on fd 0 will not block, handling signals that
    /// arrive meanwhile — or that are already pending when data is ready,
    /// so endless readable input cannot starve them. Errors other than an
    /// end or abort are left for the read(2) itself to report.
    fn wait_readable(&mut self) -> std::io::Result<()> {
        loop {
            if self.ready == 0 && !self.regular {
                self.wait_for_input()?;
            }
            if !self.signal_arrived(false)? {
                return Ok(());
            }
            // Signals were handled: check readiness again rather than
            // risk blocking in read(2), deaf to later signals.
            self.ready = 0;
        }
    }

    /// Block until fd 0 is readable (or at EOF, hung up, invalid),
    /// recording in `ready` how many bytes FIONREAD reports buffered.
    /// Signals that arrive while waiting go to `on_signal(true)`.
    fn wait_for_input(&mut self) -> std::io::Result<()> {
        let mut avail: libc::c_int = 0;
        // SAFETY: FIONREAD writes an int byte count for a valid fd; for an
        // invalid one it fails and `avail` is left untouched.
        let rc = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::FIONREAD, &mut avail) };
        if rc == 0 && avail > 0 {
            self.ready = avail as usize;
            return Ok(());
        }
        let pipe_fd = crate::signal::self_pipe_read_fd();
        loop {
            let mut fds = [
                libc::pollfd {
                    fd: libc::STDIN_FILENO,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: pipe_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: `fds` is a live two-element array for the call.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if n < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    // Poll again rather than decide here: the handler's
                    // self-pipe byte makes it return at once, and it also
                    // reports whether input arrived meanwhile.
                    continue;
                }
                return Ok(());
            }
            // Readable, at EOF, hung up or invalid: read(2) will not
            // block and reports whichever it is. Input that is ready
            // when a signal arrives wins: the read was not waiting for
            // it, so the signal is handled as one with data ready.
            if fds[0].revents != 0 {
                return Ok(());
            }
            self.signal_arrived(true)?;
        }
    }

    /// Refill `buf` with one block read. Returns the first byte, or
    /// `None` at EOF.
    fn refill_block(&mut self) -> std::io::Result<Option<u8>> {
        loop {
            self.wait_readable()?;
            self.buf.resize(READ_BLOCK, 0);
            self.pos = 0;
            // SAFETY: `buf` is a live READ_BLOCK-byte allocation owned by
            // self for the duration of the call; the length passed matches
            // exactly. See `read_one` for why a stale/closed fd 0 is safe.
            let n = unsafe {
                libc::read(
                    libc::STDIN_FILENO,
                    self.buf.as_mut_ptr() as *mut libc::c_void,
                    READ_BLOCK,
                )
            };
            if n > 0 {
                self.ready = self.ready.saturating_sub(n as usize);
                self.buf.truncate(n as usize);
                self.pos = 1;
                return Ok(Some(self.buf[0]));
            }
            self.buf.clear();
            if n == 0 {
                return Ok(None);
            }
            // n == -1: check errno
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                // Readiness is rechecked after any signal action.
                self.ready = 0;
                self.signal_arrived(true)?;
                continue;
            }
            return Err(err);
        }
    }

    /// Single-byte read(2) — the non-seekable path.
    fn read_one(&mut self) -> std::io::Result<Option<u8>> {
        let mut buf = [0u8; 1];
        loop {
            self.wait_readable()?;
            // SAFETY: buf is a valid 1-byte stack allocation that lives for
            // the duration of this call; the length matches the buffer size
            // exactly. STDIN_FILENO is a plain fd number that is valid to
            // pass at syscall time regardless of the fd's state: if the
            // user has closed fd 0 (e.g. `exec 0>&-`), libc::read returns
            // -1 with EBADF, which the error path below propagates as
            // `Err` — fail-safe, never undefined behavior.
            let n =
                unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr() as *mut libc::c_void, 1) };
            if n == 1 {
                self.ready = self.ready.saturating_sub(1);
                return Ok(Some(buf[0]));
            }
            if n == 0 {
                return Ok(None);
            }
            // n == -1: check errno
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                // Readiness is rechecked after any signal action.
                self.ready = 0;
                self.signal_arrived(true)?;
                continue;
            }
            return Err(err);
        }
    }
}

impl<F: FnMut(bool) -> SignalOutcome> ByteReader for StdinByteReader<F> {
    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        if self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            self.pos += 1;
            return Ok(Some(b));
        }
        if self.ended {
            return Ok(None);
        }
        let result = if self.seekable {
            self.refill_block()
        } else {
            self.read_one()
        };
        match result {
            // A trapped signal ended the read: the line ends here, as at
            // end-of-file, so the partial line is still assigned.
            Err(_) if self.ended => Ok(None),
            other => other,
        }
    }
}
