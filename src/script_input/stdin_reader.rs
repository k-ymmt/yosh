//! The incremental reader for commands on standard input, which never
//! reads ahead of the command being run (see the parent module).

use std::os::unix::io::RawFd;

/// Incremental reader for commands on standard input.
pub struct StdinReader {
    fd: RawFd,
    /// fd is a regular file: read blocks with `pread` and keep the fd
    /// offset at the end of the last command that ran.
    pub(super) seekable: bool,
    /// File offset of the next byte to read (seekable only).
    pub(super) read_pos: i64,
    pub(super) eof: bool,
    /// Non-seekable: the file fd referred to when `eof` was set (see
    /// `input_replaced`).
    eof_file: Option<FileId>,
    /// Non-seekable: the start of a line whose read was abandoned (the
    /// input ran dry for `fill_ready_line`, or a signal ended the
    /// read); the next read continues it.
    partial: Vec<u8>,
}

/// What the signal callback of `StdinReader` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignalCheck {
    /// No signal was pending: nothing ran.
    Quiet,
    /// Signal actions ran and reading goes on. A trap action may have
    /// read from the input itself (`trap 'read y' USR1`), so readiness
    /// seen before it ran is stale.
    Handled,
    /// Abandon the read (the shell is to exit, or an interactive SIGINT
    /// cancels the input).
    Abandon,
}

/// Identity of an open file (device and inode), to tell whether fd 0
/// was replaced (`exec <file`).
type FileId = (u64, u64);

fn file_id(fd: RawFd) -> Option<FileId> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat fills `st` for a valid fd and fails otherwise.
    (unsafe { libc::fstat(fd, &mut st) } == 0).then_some((st.st_dev as u64, st.st_ino as u64))
}

/// Block size for seekable reads.
const BLOCK: usize = 8192;

impl StdinReader {
    pub fn new() -> Self {
        Self::for_fd(libc::STDIN_FILENO)
    }

    fn for_fd(fd: RawFd) -> Self {
        let mut r = StdinReader {
            fd,
            seekable: false,
            read_pos: 0,
            eof: false,
            eof_file: None,
            partial: Vec::new(),
        };
        r.detect();
        r
    }

    /// (Re)classify the fd: a regular file with a usable offset is read
    /// in blocks, anything else byte by byte.
    pub(super) fn detect(&mut self) {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let regular = unsafe { libc::fstat(self.fd, &mut st) } == 0
            && (st.st_mode & libc::S_IFMT) == libc::S_IFREG;
        let pos = unsafe { libc::lseek(self.fd, 0, libc::SEEK_CUR) };
        self.seekable = regular && pos >= 0;
        self.read_pos = pos.max(0);
        self.eof = false;
        self.partial.clear();
    }

    /// Append more raw input to `out`. Non-seekable input gets one line
    /// (up to and including a newline); seekable input a block cut back
    /// to its last newline (at least `min` bytes when the line allows).
    /// Sets `eof` when nothing more can be read.
    ///
    /// Pending signals are handed to `on_signal` before every read of
    /// non-seekable input — while it waits, and also between reads that
    /// have data ready — so traps run (and untrapped fatal signals end
    /// the shell) without waiting for the next line, which input that is
    /// always readable but never ends a line would postpone forever.
    /// When it says to abandon the read, `fill` returns with what it has,
    /// `eof` left unset, and the start of an unfinished line is kept in
    /// `partial`.
    pub(super) fn fill(
        &mut self,
        out: &mut Vec<u8>,
        min: usize,
        on_signal: &mut dyn FnMut() -> SignalCheck,
    ) {
        if self.eof {
            return;
        }
        if self.seekable {
            self.fill_block(out, min.max(BLOCK));
        } else {
            self.fill_line(out, on_signal, true);
        }
    }

    /// Non-seekable input: append one line to `out` only if it can be
    /// read without blocking. Returns true when a whole line was
    /// appended (end of input is left to the next blocking read). A
    /// line that is only partly there is kept back and finished by the
    /// next read, so `out` never ends inside a line (or a character).
    /// Signals are handled as in `fill`.
    pub(super) fn fill_ready_line(
        &mut self,
        out: &mut Vec<u8>,
        on_signal: &mut dyn FnMut() -> SignalCheck,
    ) -> bool {
        if self.eof || self.seekable {
            return false;
        }
        let before = out.len();
        self.fill_line(out, on_signal, false);
        out.len() > before
    }

    /// Discard the start of an unfinished line kept by an abandoned read.
    pub(super) fn discard_partial_line(&mut self) {
        self.partial.clear();
    }

    /// Whether fd 0 was replaced (`exec <file`) after non-seekable input
    /// reached its end: reading goes on from the new file (dash and bash
    /// run the commands of `exec <file` on an unterminated last line).
    /// Seekable input notices a replacement by its offset instead.
    pub(super) fn input_replaced(&self) -> bool {
        self.eof && !self.seekable && file_id(self.fd) != self.eof_file
    }

    /// Read on after `input_replaced`: line by line, as before.
    pub(super) fn reopen(&mut self) {
        self.eof = false;
        self.eof_file = None;
        self.partial.clear();
    }

    /// Block until fd has input (or EOF/error), calling `on_signal`
    /// for signals delivered meanwhile. The self-pipe handlers use
    /// SA_RESTART, so a plain blocking read would sleep through them.
    ///
    /// `select`, not `poll`: macOS `poll` never reports end of input on
    /// a named FIFO once its writers are gone (nor works on some
    /// devices), which would hang `sh < fifo`.
    ///
    /// Returns false when `on_signal` asked to abandon the read.
    fn wait_readable(&self, on_signal: &mut dyn FnMut() -> SignalCheck) -> bool {
        let pipe = crate::signal::try_self_pipe_read_fd().unwrap_or(-1);
        let limit = libc::FD_SETSIZE as RawFd;
        if pipe < 0 || pipe >= limit || self.fd < 0 || self.fd >= limit {
            // Cannot wait on both: fall back to the plain blocking read.
            return true;
        }
        loop {
            // SAFETY: fd_set is plain data initialized by FD_ZERO; both
            // fds were checked to be below FD_SETSIZE.
            let (n, set) = unsafe {
                let mut set: libc::fd_set = std::mem::zeroed();
                libc::FD_ZERO(&mut set);
                libc::FD_SET(self.fd, &mut set);
                libc::FD_SET(pipe, &mut set);
                let n = libc::select(
                    self.fd.max(pipe) + 1,
                    &mut set,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                (n, set)
            };
            if n < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // EBADF etc.: let the read report the problem.
                return true;
            }
            if unsafe { libc::FD_ISSET(pipe, &set) } {
                match on_signal() {
                    SignalCheck::Abandon => return false,
                    // A trap action may have consumed the input `select`
                    // found ready (`trap 'read y' USR1`): wait again
                    // rather than block in read(2), deaf to later
                    // signals until more input arrives.
                    SignalCheck::Handled => continue,
                    SignalCheck::Quiet => {}
                }
            }
            if unsafe { libc::FD_ISSET(self.fd, &set) } {
                return true;
            }
        }
    }

    /// Read one line (continuing `partial`) and append it to `out`.
    /// With `wait`, wait for input as needed; without it, stop as soon
    /// as no more input is ready. A read abandoned either way (or by
    /// `on_signal`) keeps the bytes read so far in `partial`.
    fn fill_line(
        &mut self,
        out: &mut Vec<u8>,
        on_signal: &mut dyn FnMut() -> SignalCheck,
        wait: bool,
    ) {
        let mut line = std::mem::take(&mut self.partial);
        let mut b = [0u8; 1];
        // Bytes known to be readable without blocking (FIONREAD), so the
        // signal-aware wait is needed only when the input runs dry.
        let mut avail: libc::c_int = 0;
        loop {
            // Before every read(2), not only before waiting: input that
            // is always readable (`yes | tr -d '\n'`) never waits, and
            // would starve the signals. No syscall unless a signal
            // handler ran since the last check.
            match on_signal() {
                SignalCheck::Abandon => {
                    self.partial = line;
                    return;
                }
                // A trap action may have read the bytes FIONREAD counted:
                // check again before read(2) could block.
                SignalCheck::Handled => avail = 0,
                SignalCheck::Quiet => {}
            }
            if avail <= 0 {
                // SAFETY: FIONREAD stores an int through the pointer.
                if unsafe { libc::ioctl(self.fd, libc::FIONREAD, &mut avail) } != 0 {
                    avail = 0;
                }
                if avail <= 0 && !(wait && self.wait_readable(on_signal)) {
                    // Nothing ready, or a signal ended the read.
                    self.partial = line;
                    return;
                }
            }
            let n = unsafe { libc::read(self.fd, b.as_mut_ptr() as *mut libc::c_void, 1) };
            if n == 1 {
                avail -= 1;
                line.push(b[0]);
                if b[0] == b'\n' {
                    out.extend_from_slice(&line);
                    return;
                }
            } else if n < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            } else {
                // EOF, or an unreadable fd (`exec <&-`): end of input.
                self.eof = true;
                self.eof_file = file_id(self.fd);
                out.extend_from_slice(&line);
                return;
            }
        }
    }

    fn fill_block(&mut self, out: &mut Vec<u8>, want: usize) {
        let mut buf = vec![0u8; want];
        loop {
            let n = unsafe {
                libc::pread(
                    self.fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    want,
                    self.read_pos as libc::off_t,
                )
            };
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if n <= 0 {
                self.eof = true;
                return;
            }
            let n = n as usize;
            // Keep whole lines only, so a multi-byte character is never
            // split and the next block starts at a line boundary; a
            // block without a newline is kept whole and the line is
            // finished by the next one.
            match buf[..n].iter().rposition(|&b| b == b'\n') {
                Some(i) => {
                    out.extend_from_slice(&buf[..=i]);
                    self.read_pos += i as i64 + 1;
                    return;
                }
                None => {
                    out.extend_from_slice(&buf[..n]);
                    self.read_pos += n as i64;
                }
            }
        }
    }

    /// The fd's current offset, or `None` when it has none (or fd 0 no
    /// longer refers to a regular file).
    pub(super) fn offset(&self) -> Option<i64> {
        let pos = unsafe { libc::lseek(self.fd, 0, libc::SEEK_CUR) };
        (pos >= 0).then_some(pos)
    }

    pub(super) fn seek(&self, pos: i64) {
        unsafe { libc::lseek(self.fd, pos as libc::off_t, libc::SEEK_SET) };
    }

    /// Move the fd offset from `from` to `to` with one relative lseek,
    /// returning true. If the offset was not at `from` (or the fd lost
    /// its offset), leave it where it was and return false.
    pub(super) fn seek_from(&self, from: i64, to: i64) -> bool {
        let delta = to - from;
        let pos = unsafe { libc::lseek(self.fd, delta as libc::off_t, libc::SEEK_CUR) };
        if pos == to {
            return true;
        }
        if pos >= 0 {
            // It moved from somewhere else: put it back.
            unsafe { libc::lseek(self.fd, (pos - delta) as libc::off_t, libc::SEEK_SET) };
        }
        false
    }
}

impl Default for StdinReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn reader_over(bytes: &[u8]) -> (StdinReader, std::fs::File) {
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(bytes).unwrap();
        use std::os::unix::io::AsRawFd;
        unsafe { libc::lseek(f.as_raw_fd(), 0, libc::SEEK_SET) };
        (StdinReader::for_fd(f.as_raw_fd()), f)
    }

    #[test]
    fn regular_file_is_seekable_and_read_in_whole_lines() {
        let (mut r, _f) = reader_over(b"echo a\necho b\npartial");
        assert!(r.seekable);
        let mut out = Vec::new();
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert_eq!(out, b"echo a\necho b\n");
        assert!(!r.eof);
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert_eq!(out, b"echo a\necho b\npartial");
        assert!(r.eof);
    }

    #[test]
    fn seek_from_moves_only_from_the_expected_offset() {
        let (r, _f) = reader_over(b"0123456789");
        r.seek(3);
        assert!(r.seek_from(3, 7));
        assert_eq!(r.offset(), Some(7));
        // The offset is not where the caller left it: it stays put.
        assert!(!r.seek_from(3, 9));
        assert_eq!(r.offset(), Some(7));
        assert!(!r.seek_from(9, 0));
        assert_eq!(r.offset(), Some(7));
        // No offset at all.
        let (rd, _wr) = nix::unistd::pipe().unwrap();
        use std::os::unix::io::AsRawFd;
        assert!(!StdinReader::for_fd(rd.as_raw_fd()).seek_from(0, 1));
    }

    #[test]
    fn pipe_is_read_one_line_at_a_time() {
        let (rd, wr) = nix::unistd::pipe().unwrap();
        nix::unistd::write(&wr, b"one\ntwo\n").unwrap();
        drop(wr);
        use std::os::unix::io::AsRawFd;
        let mut r = StdinReader::for_fd(rd.as_raw_fd());
        assert!(!r.seekable);
        let mut out = Vec::new();
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert_eq!(out, b"one\n");
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert_eq!(out, b"one\ntwo\n");
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert!(r.eof);
    }

    #[test]
    fn ready_line_never_blocks_and_keeps_a_partial_line() {
        let (rd, wr) = nix::unistd::pipe().unwrap();
        nix::unistd::write(&wr, b"one\ntw\xc3").unwrap();
        use std::os::unix::io::AsRawFd;
        let mut r = StdinReader::for_fd(rd.as_raw_fd());
        let mut out = Vec::new();
        assert!(r.fill_ready_line(&mut out, &mut || SignalCheck::Quiet));
        assert_eq!(out, b"one\n");
        // Only part of the next line (ending inside a character) is
        // there: nothing is appended, and the call does not block.
        assert!(!r.fill_ready_line(&mut out, &mut || SignalCheck::Quiet));
        assert_eq!(out, b"one\n");
        nix::unistd::write(&wr, b"\xa9\nthree").unwrap();
        drop(wr);
        assert!(r.fill_ready_line(&mut out, &mut || SignalCheck::Quiet));
        assert_eq!(out, "one\ntw\u{e9}\n".as_bytes());
        // A blocking read finishes the unterminated last line at EOF.
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert_eq!(out, "one\ntw\u{e9}\nthree".as_bytes());
        assert!(r.eof);
        assert!(!r.fill_ready_line(&mut out, &mut || SignalCheck::Quiet));
    }

    #[test]
    fn signals_are_checked_between_reads_of_ready_input() {
        // Input that never ends its line must not starve signals: the
        // check runs before every read, also with data ready, and a read
        // it abandons keeps the line read so far for the next read.
        let (rd, wr) = nix::unistd::pipe().unwrap();
        nix::unistd::write(&wr, b"abcdef").unwrap();
        use std::os::unix::io::AsRawFd;
        let mut r = StdinReader::for_fd(rd.as_raw_fd());
        let mut out = Vec::new();
        let mut calls = 0;
        r.fill(&mut out, 0, &mut || {
            calls += 1;
            if calls > 3 {
                SignalCheck::Abandon
            } else {
                SignalCheck::Quiet
            }
        });
        assert!(out.is_empty());
        assert_eq!(r.partial, b"abc");
        assert!(!r.eof);
        nix::unistd::write(&wr, b"\n").unwrap();
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert_eq!(out, b"abcdef\n");
    }

    #[test]
    fn readiness_is_checked_again_after_a_signal_action() {
        // A trap action that reads the input itself (`trap 'read y'
        // USR1`) consumes bytes FIONREAD had counted: the reader must
        // look again instead of reading as if they were still there.
        // (Non-blocking here, so a regression reads EAGAIN — taken for
        // end of input — instead of hanging the test.)
        use std::os::unix::io::AsRawFd;
        let (rd, wr) = nix::unistd::pipe().unwrap();
        nix::unistd::write(&wr, b"abcdef\n").unwrap();
        let fd = rd.as_raw_fd();
        unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) };
        let mut r = StdinReader::for_fd(fd);
        let mut out = Vec::new();
        let mut calls = 0;
        let got_line = r.fill_ready_line(&mut out, &mut || {
            calls += 1;
            if calls == 2 {
                // The "trap" reads the rest of the line.
                let mut rest = [0u8; 16];
                let n = unsafe { libc::read(fd, rest.as_mut_ptr().cast(), rest.len()) };
                assert_eq!(n, 6);
                SignalCheck::Handled
            } else {
                SignalCheck::Quiet
            }
        });
        assert!(!got_line);
        assert!(out.is_empty());
        assert_eq!(r.partial, b"a");
        assert!(!r.eof);
    }

    #[test]
    fn replaced_input_is_noticed_after_end_of_input() {
        use std::os::unix::io::AsRawFd;
        let (rd, wr) = nix::unistd::pipe().unwrap();
        drop(wr);
        let fd = unsafe { libc::dup(rd.as_raw_fd()) };
        let mut r = StdinReader::for_fd(fd);
        let mut out = Vec::new();
        r.fill(&mut out, 0, &mut || SignalCheck::Quiet);
        assert!(r.eof);
        assert!(!r.input_replaced());
        // `exec <file`: the fd now refers to another file.
        let (other, _w) = nix::unistd::pipe().unwrap();
        unsafe { libc::dup2(other.as_raw_fd(), fd) };
        assert!(r.input_replaced());
        r.reopen();
        assert!(!r.eof);
        unsafe { libc::close(fd) };
    }
}
