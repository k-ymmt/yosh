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

use std::os::unix::io::RawFd;

use crate::byteenc;
use crate::env;
use crate::error::{ParseErrorKind, ShellErrorKind};
use crate::exec::Executor;
use crate::parser::Parser;

/// Where the command text comes from.
pub enum Source<'a> {
    /// The whole input is known up front (`-c` string, script file).
    Text(&'a str),
    /// Standard input, read without reading ahead.
    Stdin(StdinReader),
}

/// Incremental reader for commands on standard input.
pub struct StdinReader {
    fd: RawFd,
    /// fd is a regular file: read blocks with `pread` and keep the fd
    /// offset at the end of the last command that ran.
    seekable: bool,
    /// File offset of the next byte to read (seekable only).
    read_pos: i64,
    eof: bool,
}

/// Block size for seekable reads.
const BLOCK: usize = 8192;

/// Unconsumed command text larger than this is re-parsed only after it
/// grew by an eighth since the last incomplete attempt (non-seekable
/// input), keeping a long compound command linear to parse instead of
/// re-parsing it once per line. Below it every line is tried, so no
/// line after a complete command is read before that command runs.
const REPARSE_EVERY_LINE_LIMIT: usize = 16 * 1024;

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
        };
        r.detect();
        r
    }

    /// (Re)classify the fd: a regular file with a usable offset is read
    /// in blocks, anything else byte by byte.
    fn detect(&mut self) {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let regular = unsafe { libc::fstat(self.fd, &mut st) } == 0
            && (st.st_mode & libc::S_IFMT) == libc::S_IFREG;
        let pos = unsafe { libc::lseek(self.fd, 0, libc::SEEK_CUR) };
        self.seekable = regular && pos >= 0;
        self.read_pos = pos.max(0);
        self.eof = false;
    }

    /// Append more raw input to `out`. Non-seekable input gets one line
    /// (up to and including a newline); seekable input a block cut back
    /// to its last newline (at least `min` bytes when the line allows).
    /// Sets `eof` when nothing more can be read.
    ///
    /// While waiting for input, `on_signal` is called whenever a signal
    /// arrives on the shell's self-pipe, so traps run (and untrapped
    /// fatal signals end the shell) without waiting for the next line.
    fn fill(&mut self, out: &mut Vec<u8>, min: usize, on_signal: &mut dyn FnMut()) {
        if self.eof {
            return;
        }
        if self.seekable {
            self.fill_block(out, min.max(BLOCK));
        } else {
            self.fill_line(out, on_signal);
        }
    }

    /// Block until fd has input (or EOF/error), calling `on_signal`
    /// for signals delivered meanwhile. The self-pipe handlers use
    /// SA_RESTART, so a plain blocking read would sleep through them.
    ///
    /// `select`, not `poll`: macOS `poll` never reports end of input on
    /// a named FIFO once its writers are gone (nor works on some
    /// devices), which would hang `sh < fifo`.
    fn wait_readable(&self, on_signal: &mut dyn FnMut()) {
        let pipe = crate::signal::try_self_pipe_read_fd().unwrap_or(-1);
        let limit = libc::FD_SETSIZE as RawFd;
        if pipe < 0 || pipe >= limit || self.fd < 0 || self.fd >= limit {
            // Cannot wait on both: fall back to the plain blocking read.
            return;
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
                return;
            }
            if unsafe { libc::FD_ISSET(pipe, &set) } {
                on_signal();
            }
            if unsafe { libc::FD_ISSET(self.fd, &set) } {
                return;
            }
        }
    }

    fn fill_line(&mut self, out: &mut Vec<u8>, on_signal: &mut dyn FnMut()) {
        let mut b = [0u8; 1];
        // Bytes known to be readable without blocking (FIONREAD), so the
        // signal-aware wait is needed only when the input runs dry.
        let mut avail: libc::c_int = 0;
        loop {
            if avail <= 0 {
                // SAFETY: FIONREAD stores an int through the pointer.
                if unsafe { libc::ioctl(self.fd, libc::FIONREAD, &mut avail) } != 0 {
                    avail = 0;
                }
                if avail <= 0 {
                    self.wait_readable(on_signal);
                }
            }
            let n = unsafe { libc::read(self.fd, b.as_mut_ptr() as *mut libc::c_void, 1) };
            if n == 1 {
                avail -= 1;
                out.push(b[0]);
                if b[0] == b'\n' {
                    return;
                }
            } else if n < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            } else {
                // EOF, or an unreadable fd (`exec <&-`): end of input.
                self.eof = true;
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
    fn offset(&self) -> Option<i64> {
        let pos = unsafe { libc::lseek(self.fd, 0, libc::SEEK_CUR) };
        (pos >= 0).then_some(pos)
    }

    fn seek(&self, pos: i64) {
        unsafe { libc::lseek(self.fd, pos as libc::off_t, libc::SEEK_SET) };
    }
}

impl Default for StdinReader {
    fn default() -> Self {
        Self::new()
    }
}

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
        let mut p = Parser::new_with_aliases_at_line(&text, &executor.env.aliases, line);
        // Once the whole input is here, end of input ends an
        // unterminated here-document, as in dash and bash.
        p.set_eof_ends_heredoc(at_eof);
        let mut echoed = 0;
        // Start of the command being parsed (end of the previous one).
        let mut cmd_start = 0usize;
        // Raw-byte bookkeeping for seekable stdin: encoded position
        // `enc_mark` is raw offset `raw_mark` from text_raw_start.
        let (mut enc_mark, mut raw_mark) = (0usize, 0usize);

        loop {
            p.set_aliases(&executor.env.aliases);
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
            if needs_more {
                let reader = reader.as_mut().expect("needs_more implies a reader");
                // Drop the executed commands and read more after the rest.
                line += text[..cmd_start].bytes().filter(|&b| b == b'\n').count();
                if reader.seekable {
                    text_raw_start += raw_mark as i64
                        + byteenc::decode_bytes(&text[enc_mark..cmd_start]).len() as i64;
                }
                text.drain(..cmd_start);
                let mut raw = Vec::new();
                if reader.seekable {
                    reader.fill(&mut raw, text.len(), &mut || {});
                } else {
                    // An interactive shell (`sh -i` on a non-terminal)
                    // prompts for each line: PS1 at a command start, PS2
                    // for a continuation line (dash and bash agree).
                    let mut ps1 = matches!(parsed, Ok(None));
                    let mut read_line = |raw: &mut Vec<u8>| {
                        if executor.env.mode.is_interactive {
                            if ps1 {
                                // Job status changes are reported before
                                // the PS1 prompt, as in the REPL.
                                executor.reap_zombies();
                                executor.display_job_notifications();
                            }
                            write_prompt(executor, if ps1 { "PS1" } else { "PS2" });
                            ps1 = false;
                        }
                        reader.fill(raw, 0, &mut || executor.process_pending_signals());
                        !reader.eof
                    };
                    // Read lines until the pending text is long enough to
                    // be worth re-parsing (every line while it is short).
                    if text.len() < retry_len {
                        while text.len() + raw.len() < retry_len && read_line(&mut raw) {}
                    } else {
                        read_line(&mut raw);
                    }
                    let len = text.len() + raw.len();
                    retry_len = if len > REPARSE_EVERY_LINE_LIMIT {
                        len + len / 8
                    } else {
                        0
                    };
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
                    // The command read from (or replaced) stdin: continue
                    // reading where it left off.
                    if let (Some(r), Some(off)) = (reader.as_mut(), expected)
                        && r.offset() != Some(off)
                    {
                        line += text[..end].bytes().filter(|&b| b == b'\n').count();
                        text.clear();
                        r.detect();
                        text_raw_start = r.read_pos;
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
                    let from = p.input_position().saturating_sub(1).max(cmd_start);
                    let end = text[from..].find('\n').map_or(text.len(), |i| from + i + 1);
                    line += text[..end].bytes().filter(|&b| b == b'\n').count();
                    // (Interactive stdin is never read in seekable block
                    // mode, so there is no file offset to adjust.)
                    text.drain(..end);
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
    status
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
        r.fill(&mut out, 0, &mut || {});
        assert_eq!(out, b"echo a\necho b\n");
        assert!(!r.eof);
        r.fill(&mut out, 0, &mut || {});
        assert_eq!(out, b"echo a\necho b\npartial");
        assert!(r.eof);
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
        r.fill(&mut out, 0, &mut || {});
        assert_eq!(out, b"one\n");
        r.fill(&mut out, 0, &mut || {});
        assert_eq!(out, b"one\ntwo\n");
        r.fill(&mut out, 0, &mut || {});
        assert!(r.eof);
    }
}
