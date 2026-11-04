//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

use crate::builtin::options::{invalid_flag, scan_flag_words};
use crate::env::{FlowControl, TrapAction};
use crate::error::ShellError;
use crate::exec::Executor;
use crate::parser::word::is_valid_name;

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

    let (line_result, interrupted) = {
        let mut reader =
            StdinByteReader::new(|waiting| handle_signals_during_read(executor, waiting));
        let line_result = read_logical_line(parsed.raw, &mut reader);
        // Seek fd 0 back over any block-buffered bytes beyond the consumed
        // line, on success AND error paths, so the next reader of the fd
        // starts exactly where this `read` logically stopped.
        reader.rewind_unconsumed();
        (line_result, reader.interrupted)
    };
    // A signal ended the read (see `handle_signals_during_read`): no
    // variable is assigned and the status reports the signal.
    if let Some(status) = interrupted {
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

/// Block size for buffered reads from seekable input. One page is plenty:
/// the goal is to amortize the read(2)-per-byte syscall cost (~40
/// syscalls/line before), not to stream large volumes.
const READ_BLOCK: usize = 4096;

/// Run the trap or default action of the signals that arrived during
/// `read`, and decide whether the read goes on. Returns the status to
/// abort with, or `None` to keep reading. `waiting` tells whether the
/// read is blocked on idle input or only between read(2) calls that
/// have data ready.
///
/// Untrapped fatal signals end a non-interactive shell right here (inside
/// `run_signal_traps`), instead of only once a line completes — which
/// never happens for endless input without a newline (`read x
/// </dev/zero`). As in dash and `bash --posix`, a trapped signal that
/// arrives while the read waits runs its action and then ends the read
/// with status 128+N, so `trap ... ALRM` can time out a `read`; with data
/// ready the action runs and the read goes on (dash keeps reading). An
/// interactive shell's untrapped SIGINT abandons the read (130) either
/// way, and with it the rest of the command line — like Ctrl+C on a
/// foreground job in dash and bash (`read x; echo WRONG` prints nothing).
fn handle_signals_during_read(executor: &mut Executor, waiting: bool) -> Option<i32> {
    // No syscall unless a handler has fired since the last drain: this
    // runs before every read(2) of a busy read.
    let signals = crate::signal::drain_pending_signals();
    if signals.is_empty() {
        return None;
    }
    let traps = &executor.env.traps;
    let trapped = signals.iter().copied().rev().find(|&sig| {
        sig != libc::SIGCHLD && matches!(traps.get_signal_trap(sig), Some(TrapAction::Command(_)))
    });
    let interactive_int = executor.env.mode.is_interactive
        && signals.contains(&libc::SIGINT)
        && !matches!(
            traps.get_signal_trap(libc::SIGINT),
            Some(TrapAction::Command(_) | TrapAction::Ignore)
        );
    executor.run_signal_traps(&signals);
    if let Some(sig) = trapped.filter(|_| waiting) {
        return Some(128 + sig);
    }
    if interactive_int {
        // Unwind to the prompt the way a fatal expansion error does in an
        // interactive shell: every list, compound command and function
        // stops (`should_unwind`), and the REPL / `-c` loop clears the
        // flag before the next command line.
        if executor.env.exec.flow_control.is_none() {
            executor.env.exec.flow_control = Some(FlowControl::ExpansionError);
        }
        return Some(128 + libc::SIGINT);
    }
    if executor.should_unwind() {
        return Some(128 + signals[signals.len() - 1]);
    }
    None
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
/// atomic flag test when no signal is pending.
struct StdinByteReader<F: FnMut(bool) -> Option<i32>> {
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
    /// waiting for input); `Some(status)` aborts the read.
    on_signal: F,
    /// The abort status `on_signal` returned, if it ended the read.
    interrupted: Option<i32>,
}

impl<F: FnMut(bool) -> Option<i32>> StdinByteReader<F> {
    fn new(on_signal: F) -> Self {
        // A successful lseek means the fd has a file offset we can
        // rewind; ESPIPE (pipe/FIFO/socket) and failures in general mean
        // it does not — use the conservative per-byte path.
        let seekable = unsafe { libc::lseek(libc::STDIN_FILENO, 0, libc::SEEK_CUR) } != -1;
        StdinByteReader {
            seekable,
            buf: Vec::new(),
            pos: 0,
            ready: 0,
            on_signal,
            interrupted: None,
        }
    }

    /// Seek fd 0 back over the buffered-but-unconsumed bytes so the file
    /// offset lands exactly after the last byte this reader handed out
    /// (i.e. just past the consumed newline, or at EOF). No-op for
    /// non-seekable input and when the buffer is fully drained.
    fn rewind_unconsumed(&mut self) {
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

    /// Let `on_signal` handle pending signals; turns an abort into an
    /// `Interrupted` error that ends `read_logical_line`.
    fn signal_arrived(&mut self, waiting: bool) -> std::io::Result<()> {
        if let Some(status) = (self.on_signal)(waiting) {
            self.interrupted = Some(status);
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        Ok(())
    }

    /// Wait until a read(2) on fd 0 will not block, handling signals that
    /// arrive meanwhile — or that are already pending when data is ready,
    /// so endless readable input cannot starve them. Errors other than an
    /// abort are left for the read(2) itself to report.
    fn wait_readable(&mut self) -> std::io::Result<()> {
        if self.ready > 0 {
            return self.signal_arrived(false);
        }
        let mut avail: libc::c_int = 0;
        // SAFETY: FIONREAD writes an int byte count for a valid fd; for an
        // invalid one it fails and `avail` is left untouched.
        let rc = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::FIONREAD, &mut avail) };
        if rc == 0 && avail > 0 {
            self.ready = avail as usize;
            return self.signal_arrived(false);
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
            if n < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return Ok(());
            }
            // Readable, at EOF, hung up or invalid: read(2) will not
            // block and reports whichever it is.
            if n > 0 && fds[0].revents != 0 {
                return self.signal_arrived(false);
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
                self.signal_arrived(true)?;
                continue;
            }
            return Err(err);
        }
    }
}

impl<F: FnMut(bool) -> Option<i32>> ByteReader for StdinByteReader<F> {
    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        if self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            self.pos += 1;
            return Ok(Some(b));
        }
        if self.seekable {
            self.refill_block()
        } else {
            self.read_one()
        }
    }
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

/// Assemble a field's bytes into a `String`, decoding as UTF-8 so
/// multi-byte input (e.g. `café`) is preserved. Invalid sequences are
/// preserved losslessly via the byteenc escape encoding.
fn field_to_string(slice: &[LineByte]) -> String {
    let bytes: Vec<u8> = slice.iter().map(|b| b.value).collect();
    crate::byteenc::encode_bytes(&bytes).into_owned()
}

/// One character of the logical line: a valid UTF-8 sequence, or a
/// single byte that does not start one. IFS characters match whole
/// units, so a multibyte IFS character is one delimiter (as in field
/// splitting) and a raw IFS byte matches only that raw input byte.
#[derive(Debug, Clone, Copy)]
struct Unit {
    start: usize,
    end: usize,
    /// The unit's first byte came through a backslash escape.
    escaped: bool,
}

fn line_units(line: &[LineByte]) -> Vec<Unit> {
    let mut units = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        // Only unescaped continuation bytes can extend a sequence.
        let mut buf = [0u8; 4];
        let mut n = 0;
        for (k, b) in line[i..].iter().take(4).enumerate() {
            if k > 0 && b.escaped {
                break;
            }
            buf[k] = b.value;
            n = k + 1;
        }
        let end = i + crate::byteenc::utf8_unit_len(&buf[..n]);
        units.push(Unit {
            start: i,
            end,
            escaped: line[i].escaped,
        });
        i = end;
    }
    units
}

/// Byte classes of `split_fields`'s IFS table.
const NOT_IFS: u8 = 0;
const WS_IFS: u8 = 1;
const SEP_IFS: u8 = 2;

/// POSIX §2.6.5 field splitting for `read`. Returns exactly `n_vars`
/// strings: var[0..n-1] are split fields, var[n-1] is the trailing
/// remainder (with only trailing whitespace-IFS trimmed). When IFS is
/// empty, no splitting occurs.
fn split_fields(ifs: &str, line: &[LineByte], n_vars: usize) -> Vec<String> {
    // The only caller (builtin_read) guarantees at least one var name
    // via ArgError::NoVarName.
    debug_assert!(n_vars >= 1);

    // Classify IFS characters. IFS is segmented exactly like the input
    // line (`line_units`), so a character held as several escape
    // characters is still one delimiter and a lone raw byte matches only
    // that raw input byte. One-byte characters (all IFS whitespace, and
    // every character of an ASCII IFS) go in a byte table; the rare
    // multi-byte ones are compared unit by unit.
    let mut class = [NOT_IFS; 256];
    let mut sep_multi: Vec<Vec<u8>> = Vec::new();
    let mut classify = |c: &[u8]| match c {
        [b' ' | b'\t' | b'\n'] => class[c[0] as usize] = WS_IFS,
        [b] => class[*b as usize] = SEP_IFS,
        _ => sep_multi.push(c.to_vec()),
    };
    if ifs.is_ascii() {
        for b in ifs.bytes() {
            classify(&[b]);
        }
    } else {
        for c in crate::byteenc::raw_char_units(ifs) {
            classify(&c);
        }
    }

    // Empty IFS → no splitting at all.
    if ifs.is_empty() {
        let whole = field_to_string(line);
        let mut out = vec![whole];
        out.extend((1..n_vars).map(|_| String::new()));
        return out;
    }

    let units = line_units(line);
    let unit_class = |u: &Unit| -> u8 {
        if u.escaped {
            NOT_IFS
        } else if u.end - u.start == 1 {
            class[line[u.start].value as usize]
        } else if sep_multi.iter().any(|c| {
            c.len() == u.end - u.start
                && line[u.start..u.end]
                    .iter()
                    .zip(c)
                    .all(|(b, x)| b.value == *x)
        }) {
            SEP_IFS
        } else {
            NOT_IFS
        }
    };
    // Helper: is unit `u` an unescaped IFS character of the given class?
    let is_ws = |u: &Unit| unit_class(u) == WS_IFS;
    let is_sep = |u: &Unit| unit_class(u) == SEP_IFS;
    let is_any_ifs = |u: &Unit| unit_class(u) != NOT_IFS;
    // Skip one field terminator starting at `k` and return the index
    // after it. Per XCU §2.6.5 a delimiter is a non-whitespace IFS
    // character together with any adjacent IFS whitespace (on BOTH
    // sides), or a run of IFS whitespace alone: `ws* sep? ws*`.
    let skip_terminator = |units: &[Unit], mut k: usize| -> usize {
        while k < units.len() && is_ws(&units[k]) {
            k += 1;
        }
        if k < units.len() && is_sep(&units[k]) {
            k += 1;
            while k < units.len() && is_ws(&units[k]) {
                k += 1;
            }
        }
        k
    };
    // The text of units[s..e].
    let span = |units: &[Unit], s: usize, e: usize| -> String {
        if s >= e {
            String::new()
        } else {
            field_to_string(&line[units[s].start..units[e - 1].end])
        }
    };

    // Trim leading ws_ifs.
    let mut i = 0;
    while i < units.len() && is_ws(&units[i]) {
        i += 1;
    }

    let mut result: Vec<String> = Vec::with_capacity(n_vars);

    // Emit fields 0..n_vars-2 (each terminated by IFS).
    for _ in 0..(n_vars - 1) {
        if i >= units.len() {
            result.push(String::new());
            continue;
        }
        // Field characters: until the next IFS or end-of-line.
        let start = i;
        while i < units.len() && !is_any_ifs(&units[i]) {
            i += 1;
        }
        result.push(span(&units, start, i));

        // Consume one terminator (IFS whitespace on either side of an
        // optional non-whitespace delimiter collapses into it).
        i = skip_terminator(&units, i);
    }

    // Remainder for var[n_vars-1]: trim trailing ws_ifs.
    let mut j = units.len();
    while j > i && is_ws(&units[j - 1]) {
        j -= 1;
    }
    let rest = &units[i..j];
    // If the remainder is exactly one field plus one terminator (`b:` in
    // `a:b:`), that delimiter only ends the last field — field splitting
    // yields no extra field for it — so the var gets the bare field, as
    // in dash/bash. With more fields left over (`b::`, `b:c:`), the
    // whole remainder, delimiters included, is assigned.
    let mut field_end = 0;
    while field_end < rest.len() && !is_any_ifs(&rest[field_end]) {
        field_end += 1;
    }
    let remainder = if skip_terminator(rest, field_end) == rest.len() {
        span(rest, 0, field_end)
    } else {
        span(rest, 0, rest.len())
    };
    result.push(remainder);

    debug_assert_eq!(result.len(), n_vars);
    result
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

    fn split_for(ifs: &str, line: Vec<LineByte>, n_vars: usize) -> Vec<String> {
        split_fields(ifs, &line, n_vars)
    }

    fn to_line(s: &str) -> Vec<LineByte> {
        s.bytes().map(|b| lb(b, false)).collect()
    }

    #[test]
    fn split_n_eq_1_trims_both_sides() {
        let out = split_for(" \t\n", to_line("   hello   "), 1);
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn split_n_eq_1_empty_input_yields_empty_string() {
        let out = split_for(" \t\n", to_line(""), 1);
        assert_eq!(out, vec!["".to_string()]);
    }

    #[test]
    fn split_n_gt_1_first_fields_then_remainder() {
        let out = split_for(" \t\n", to_line("a b c"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn split_remainder_keeps_internal_ifs() {
        let out = split_for(" \t\n", to_line("a b c d"), 2);
        assert_eq!(out, vec!["a".to_string(), "b c d".to_string()]);
    }

    #[test]
    fn split_leading_ifs_is_stripped() {
        let out = split_for(" \t\n", to_line("   a b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_trailing_ws_ifs_stripped_from_remainder() {
        let out = split_for(" \t\n", to_line("a b c   "), 2);
        assert_eq!(out, vec!["a".to_string(), "b c".to_string()]);
    }

    #[test]
    fn split_more_vars_than_fields_yields_empty_strings() {
        let out = split_for(" \t\n", to_line("a"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "".to_string()]);
    }

    #[test]
    fn split_empty_ifs_no_split() {
        let out = split_for("", to_line("a b c"), 2);
        // No splitting at all → entire line in var[0], var[1] empty.
        assert_eq!(out, vec!["a b c".to_string(), "".to_string()]);
    }

    #[test]
    fn split_sep_ifs_treated_as_single_separator() {
        // `IFS=:`, input "a::b" → x=a, y="" (one colon separator), z=b
        let out = split_for(":", to_line("a::b"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_mixed_sep_and_ws_ifs() {
        // IFS=":\t ", input "a: b" → ":" consumes one separator, then
        // " " is also IFS and is consumed greedily as adjacent ws.
        let out = split_for(": \t", to_line("a: b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_n_eq_1_sep_only_ifs_leading_sep_not_trimmed() {
        // IFS=":" has no whitespace-IFS bytes, so the N=1 path trims
        // nothing: the leading ':' stays (bash/dash: `IFS=: read x`
        // with input ":a" gives x=":a").
        let out = split_for(":", to_line(":a"), 1);
        assert_eq!(out, vec![":a".to_string()]);
    }

    #[test]
    fn split_n_eq_3_collapses_multi_space_runs() {
        // Runs of whitespace-IFS collapse into a single separator.
        let out = split_for(" \t\n", to_line("a   b   c"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn split_escaped_leading_sep_ifs_byte_stays_in_field_one() {
        // Non-raw input "\:a" with IFS=":": the ':' arrived escaped, so
        // it is not an IFS separator and stays in field 1.
        let line = vec![
            lb(b':', true), // escaped ':'
            lb(b'a', false),
        ];
        let out = split_fields(":", &line, 2);
        assert_eq!(out, vec![":a".to_string(), "".to_string()]);
    }

    #[test]
    fn split_escaped_byte_not_treated_as_ifs() {
        // Non-raw "a\ b": split would normally see " " as IFS, but the
        // space is escaped (came from `\<space>`), so it stays in field 1.
        let line = vec![
            lb(b'a', false),
            lb(b' ', true), // escaped space
            lb(b'b', false),
        ];
        let out = split_fields(" \t\n", &line, 2);
        assert_eq!(out, vec!["a b".to_string(), "".to_string()]);
    }

    #[test]
    fn split_ws_before_sep_collapses_into_terminator() {
        // `IFS=" :"`, "a :b": " :" is ONE delimiter (dash/bash [a][b][]).
        let out = split_for(" :", to_line("a :b"), 3);
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "".to_string()]);
        let out = split_for(" :", to_line("a : b"), 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
        // Two separators are still two delimiters.
        let out = split_for(" :", to_line("a: :b"), 3);
        assert_eq!(out, vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_last_var_drops_single_trailing_delimiter() {
        let t = |ifs: &str, line: &str, n: usize| split_for(ifs, to_line(line), n);
        assert_eq!(t(":", "a:b:", 2), vec!["a", "b"]);
        assert_eq!(t(": ", "a:b : ", 2), vec!["a", "b"]);
        assert_eq!(t(": ", "a::", 2), vec!["a", ""]);
        assert_eq!(t(": ", "a : :", 2), vec!["a", ""]);
        assert_eq!(t(": ", "a:", 1), vec!["a"]);
        assert_eq!(t(": ", " a :", 1), vec!["a"]);
        // More than one field left: delimiters are kept verbatim.
        assert_eq!(t(":", "a:b::", 2), vec!["a", "b::"]);
        assert_eq!(t(":", "a:b:c:", 2), vec!["a", "b:c:"]);
        assert_eq!(t(": ", "a b c:", 2), vec!["a", "b c:"]);
        assert_eq!(t(": ", "a:b:", 1), vec!["a:b:"]);
    }

    #[test]
    fn split_on_raw_byte_ifs() {
        // IFS holds the byteenc escape for byte 0xFF.
        let ifs = crate::byteenc::escape_char(0xFF).to_string();
        let line: Vec<LineByte> = b"a\xFFb".iter().map(|&b| lb(b, false)).collect();
        let out = split_for(&ifs, line, 2);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_multibyte_ifs_is_one_delimiter() {
        // `é` (C3 A9) splits only on the whole character, never on a
        // lone byte of it: `\xC3\xA8` (è) shares the lead byte.
        let out = split_for("é", to_line("aébèc"), 3);
        assert_eq!(
            out,
            vec!["a".to_string(), "bèc".to_string(), "".to_string()]
        );
        let out = split_for("é", to_line("aébéc"), 2);
        assert_eq!(out, vec!["a".to_string(), "béc".to_string()]);
    }

    #[test]
    fn split_escaped_multibyte_ifs_is_literal() {
        let mut line = to_line("aéb");
        line[1].escaped = true;
        let out = split_for("é", line, 2);
        assert_eq!(out, vec!["aéb".to_string(), "".to_string()]);
    }

    #[test]
    fn read_line_drops_nul_bytes() {
        let mut r = SliceReader::new(b"a\0b\\\0c\n");
        let res = read_logical_line(false, &mut r).unwrap();
        let bytes: Vec<u8> = res.bytes.iter().map(|b| b.value).collect();
        assert_eq!(bytes, b"abc");
    }
}
