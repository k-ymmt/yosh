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

/// Read output that arrives between the previously-sent command and the
/// next `$ ` prompt, returning the captured text with the trailing prompt
/// stripped.
///
/// Use this after `session.send_line("...")` to capture the command's
/// stdout. Caller is responsible for asserting on the returned string —
/// note that the captured output includes the command echo at the start
/// (e.g. `"echo foo\r\nfoo\r\n"`), and assertions should typically be
/// substring (`out.contains("foo")`) rather than equality.
///
/// When yosh's LineEditor is running in syntax-highlight mode, the echo
/// portion may include ANSI escape sequences (`\x1b[...m`); strip them
/// before substring-matching if your test cares about colored output.
pub fn read_until_prompt(session: &mut OsSession) -> String {
    let captured = session
        .expect(Regex(r"\$ "))
        .expect("next prompt not found");
    String::from_utf8_lossy(captured.before()).into_owned()
}
