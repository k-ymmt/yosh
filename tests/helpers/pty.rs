//! Shared PTY helpers for integration tests that drive yosh through a
//! pseudo-terminal (`tests/pty_interactive.rs`, `tests/pty_posix.rs`).
//!
//! The constants and helpers here are the minimal surface that both
//! consumers need: spawning yosh under expectrl, synchronizing on the
//! prompt + raw-mode transition, and reading delimited output. Test-
//! specific helpers (UI-edit-action drivers, suspend/resume sequences,
//! ANSI strippers for syntax-highlight tests) stay in their owning
//! file.

use std::os::fd::AsRawFd;
use std::process::Command;
use std::time::{Duration, Instant};

use expectrl::{Expect, Regex, Session, session::OsSession};

use super::TempDir;

pub const TIMEOUT: Duration = Duration::from_secs(15);
pub const RAW_MODE_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Spawn yosh under a pseudo-terminal with `TERM=dumb` and `HOME` pointing
/// at a fresh per-test temp directory. Returns the expectrl session plus
/// the temp dir (which must outlive the session — drop frees `HOME`).
pub fn spawn_yosh() -> (OsSession, TempDir) {
    let bin = env!("CARGO_BIN_EXE_yosh");
    let tmpdir = TempDir::new();

    let mut cmd = Command::new(bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());

    let mut session = Session::spawn(cmd).expect("failed to spawn yosh");
    session.set_expect_timeout(Some(TIMEOUT));
    (session, tmpdir)
}

/// Variant of [`spawn_yosh`] that allows the caller to override or remove
/// environment variables before exec. Used by tests that need to start with
/// `PS1` absent from the environment, an explicit `FCEDIT` value, etc.
pub fn spawn_yosh_with_env(overrides: &[(&str, Option<&str>)]) -> (OsSession, TempDir) {
    let bin = env!("CARGO_BIN_EXE_yosh");
    let tmpdir = TempDir::new();

    let mut cmd = Command::new(bin);
    cmd.env("TERM", "dumb");
    cmd.env("HOME", tmpdir.path());
    for (k, v) in overrides {
        match v {
            Some(value) => {
                cmd.env(k, value);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }

    let mut session = Session::spawn(cmd).expect("failed to spawn yosh");
    session.set_expect_timeout(Some(TIMEOUT));
    (session, tmpdir)
}

/// Wait until yosh prints `$ ` and has switched the slave PTY to raw mode.
pub fn wait_for_prompt(session: &mut OsSession) {
    session.expect("$ ").expect("prompt not found");
    wait_for_raw_mode(session);
}

/// Wait for the PS2 (`> `) continuation prompt.
pub fn wait_for_ps2(session: &mut OsSession) {
    session.expect("> ").expect("PS2 prompt not found");
    wait_for_raw_mode(session);
}

/// Block until yosh has called `enable_raw_mode()` on the PTY slave.
///
/// The previous implementation used a fixed 50ms sleep, which is a classic
/// flaky-test pattern — under load the child can take longer than that to
/// transition from canonical to raw mode, and input sent in the race window
/// gets processed by the cooked line discipline (ICRNL translation, ECHO,
/// ICANON buffering) instead of yosh's LineEditor.
///
/// Both ends of a PTY share one termios struct, so `tcgetattr` on the master
/// fd (which expectrl exposes via `AsRawFd`) observes the slave-side settings.
/// Poll for `ICANON` cleared — raw mode disables it — and return as soon as
/// the transition is visible.
pub fn wait_for_raw_mode(session: &OsSession) {
    let fd = session.as_raw_fd();
    let deadline = Instant::now() + RAW_MODE_WAIT_TIMEOUT;
    loop {
        // SAFETY: libc::termios is a C POD with no niche values; the zero
        // bit pattern is valid for every member, so zeroed() returns a
        // sound (if uninitialized-from-the-kernel's-perspective) value.
        // The very next line passes &mut termios to tcgetattr which writes
        // a full struct, so the temporarily-meaningless contents are not
        // observed.
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is the master PTY fd that expectrl owns for the
        // lifetime of `session`. tcgetattr only reads fd metadata and
        // writes into the pointed-to termios; both pointers are valid.
        let rc = unsafe { libc::tcgetattr(fd, &mut termios) };
        if rc == 0 && (termios.c_lflag & (libc::ICANON as libc::tcflag_t)) == 0 {
            return;
        }
        if Instant::now() >= deadline {
            let errno = if rc != 0 {
                std::io::Error::last_os_error().to_string()
            } else {
                "ok".to_string()
            };
            panic!(
                "wait_for_raw_mode timed out: tcgetattr rc={} ({}), c_lflag=0x{:x}",
                rc, errno, termios.c_lflag,
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Strip ANSI escape sequences (CSI `ESC [ ... <final>`, OSC
/// `ESC ] ... BEL/ST`, and two-byte `ESC <char>` sequences) from raw
/// PTY output. Printable text and CR/LF are preserved.
pub fn strip_ansi(raw: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if b == 0x1b {
            i += 1;
            match raw.get(i) {
                // CSI: ESC [ <params/intermediates 0x20-0x3f> <final 0x40-0x7e>
                Some(b'[') => {
                    i += 1;
                    while i < raw.len() && !(0x40..=0x7e).contains(&raw[i]) {
                        i += 1;
                    }
                    i += 1; // consume final byte (or run off the end)
                }
                // OSC: ESC ] ... terminated by BEL or ST (ESC \)
                Some(b']') => {
                    i += 1;
                    while i < raw.len() {
                        if raw[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if raw[i] == 0x1b && raw.get(i + 1) == Some(&b'\\') {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                // Two-byte escape (ESC 7, ESC 8, ESC =, ...)
                Some(_) => i += 1,
                None => {}
            }
        } else {
            // Preserve CR/LF and printable bytes; drop other C0 controls
            // (backspace-based repaints would otherwise corrupt matching).
            if b == b'\r' || b == b'\n' || b == b'\t' || b >= 0x20 {
                out.push(b as char);
            }
            i += 1;
        }
    }
    out
}

/// How long the PTY must stay quiet after a trailing `$ ` before we
/// accept it as the *idle* prompt rather than a transient repaint that
/// is still being written.
const PROMPT_IDLE_GRACE: Duration = Duration::from_millis(60);

/// Read output that arrives between the previously-sent command and the
/// next **idle** `$ ` prompt, returning the ANSI-stripped captured text
/// with the trailing prompt removed.
///
/// yosh's line editor repaints `$ <partial>` after every keystroke under
/// syntax highlighting, so a plain `\$ ` regex would bind to a transient
/// repaint and return prematurely. This implementation instead
/// accumulates raw PTY bytes, strips ANSI escape sequences, and only
/// returns once the stripped buffer *ends with* `$ ` and the PTY has
/// stayed quiet for a short grace window — a transient repaint ends with
/// the partial input text (`$ echo fo`), not with the bare prompt, and
/// mid-repaint prompt bytes are followed by more output within the grace
/// window.
///
/// Use this after `session.send_line("...")` to capture the command's
/// stdout. The captured text includes the command echo at the start, so
/// assertions should typically be substring or `\nfoo`-anchored rather
/// than equality. Unlike the historical implementation, the returned
/// text is already ANSI-stripped. For step-wise scripted interaction,
/// [`capture_until_sentinel`] remains the more precise primitive.
pub fn read_until_prompt(session: &mut OsSession) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match session.try_read(&mut chunk) {
            Ok(0) => panic!(
                "EOF while waiting for idle prompt; captured so far: {:?}",
                strip_ansi(&raw)
            ),
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                // Keep draining while bytes are flowing.
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let stripped = strip_ansi(&raw);
                if stripped.ends_with("$ ") {
                    // Tentative idle prompt — confirm the PTY stays
                    // quiet so we don't accept a mid-repaint `$ `.
                    std::thread::sleep(PROMPT_IDLE_GRACE);
                    match session.try_read(&mut chunk) {
                        Ok(n) if n > 0 => {
                            raw.extend_from_slice(&chunk[..n]);
                            continue;
                        }
                        _ => {
                            let end = stripped.len() - 2;
                            return stripped[..end].to_string();
                        }
                    }
                }
                if Instant::now() >= deadline {
                    panic!(
                        "timed out waiting for idle prompt; captured so far: {:?}",
                        stripped
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("read error while waiting for idle prompt: {}", e),
        }
    }
}

/// Send `cmd` followed by `; echo __YOSH_DONE__` and capture everything
/// up to the sentinel marker on a fresh line.
///
/// Use this instead of [`read_until_prompt`] when running under yosh's
/// interactive line editor: the line editor repaints the prompt on every
/// keystroke under syntax-highlighting, so a plain `$ ` regex match
/// would race past the actual command output and bind to a transient
/// repaint. The sentinel pattern eliminates that race because the
/// sentinel can only appear on a real output line.
///
/// Returns the captured output (everything between the command echo and
/// the sentinel marker, exclusive). Internally resyncs to the next `$ `
/// prompt before returning so the caller can immediately send another
/// command.
pub fn capture_until_sentinel(session: &mut OsSession, cmd: &str) -> String {
    // `; echo __YOSH_DONE__` runs after `cmd` regardless of `cmd`'s exit
    // status (we use ; not &&). The `\r?\n__YOSH_DONE__` regex matches a
    // CR/LF prefix to skip the user's echoed input which contains
    // `__YOSH_DONE__` as part of the line-editor's repaint of the
    // typed string.
    let full = format!("{}; echo __YOSH_DONE__", cmd);
    session.send_line(&full).unwrap();
    let captured = session
        .expect(Regex(r"\r?\n__YOSH_DONE__"))
        .expect("sentinel __YOSH_DONE__ not found");
    let out = String::from_utf8_lossy(captured.before()).into_owned();
    // Resync: consume up to and including the next prompt.
    let _ = session.expect(Regex(r"\$ ")).expect("prompt not found");
    out
}

/// Variant of [`capture_until_sentinel`] that routes the sentinel echo
/// to fd 2 (`; echo __YOSH_DONE__ >&2`).
///
/// Use this for step-wise interaction across an `exec >file` boundary:
/// once the shell's fd 1 is redirected to a file, a plain
/// `echo __YOSH_DONE__` lands in the file and [`capture_until_sentinel`]
/// hangs waiting for a sentinel that never reaches the PTY. The sentinel
/// travelling on fd 2 still reaches the PTY (yosh writes its prompt on
/// stderr too, so the post-sentinel prompt resync also works).
///
/// Note the portable POSIX form `echo ... >&2` (redirection after the
/// command), not the bash-ism `>&2 echo ...`.
///
/// Anchoring differs from [`capture_until_sentinel`]: while fd 1 is
/// redirected, the line editor's echo of the typed command goes to the
/// redirect target instead of the PTY, so the sentinel may be the very
/// first byte the PTY sees — a `\r?\n` prefix anchor would never match.
/// Instead the *typed* sentinel is quote-split (`__YOSH_D"ONE__"`) so the
/// contiguous marker string can only ever appear as real command output,
/// never inside the echoed input, and no newline anchor is needed.
pub fn capture_until_sentinel_via_stderr(session: &mut OsSession, cmd: &str) -> String {
    let full = format!("{}; echo __YOSH_D\"ONE__\" >&2", cmd);
    session.send_line(&full).unwrap();
    let captured = session
        .expect("__YOSH_DONE__")
        .expect("sentinel __YOSH_DONE__ not found on stderr");
    let out = String::from_utf8_lossy(captured.before()).into_owned();
    // Resync: the prompt is printed on stderr, so it reaches the PTY
    // even while fd 1 is redirected elsewhere.
    let _ = session.expect(Regex(r"\$ ")).expect("prompt not found");
    out
}

/// Run a command via the sentinel pattern and discard the output. Used
/// when only the side effects (e.g. `export FOO=bar`) matter and the
/// caller does not need to inspect what the command printed.
pub fn run_and_drain(session: &mut OsSession, cmd: &str) {
    let _ = capture_until_sentinel(session, cmd);
}
