// Regression test: an interactive yosh whose controlling pty master
// closes (terminal-emulator crash, test-harness teardown) must exit
// like Ctrl+D instead of busy-spinning at 100% CPU.
//
// The bug lived in `CrosstermTerminal::read_event`: after the master
// closes, the slave fd polls as readable while `read` returns 0 bytes,
// and crossterm's internal tty loop does not handle that EOF — any
// `event::poll` after that point spun forever. The fix probes the fd
// for hangup (POLLHUP, or POLLIN with zero buffered bytes) before every
// crossterm poll and surfaces it as EOF.
//
// expectrl is deliberately not used here: its teardown kills the child,
// which is exactly what this test must not do. The pty pair is built
// with raw libc calls instead.

use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod helpers;
use helpers::TempDir;

/// Open a pty pair, returning (master, slave).
fn open_pty() -> (OwnedFd, OwnedFd) {
    // SAFETY: standard posix_openpt/grantpt/unlockpt/open sequence; fds
    // are checked before being wrapped.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(master >= 0, "posix_openpt failed");
        assert_eq!(libc::grantpt(master), 0, "grantpt failed");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");
        // ptsname's static buffer is fine: this test binary opens ptys
        // from a single thread.
        let name = libc::ptsname(master);
        assert!(!name.is_null(), "ptsname failed");
        let slave = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
        assert!(slave >= 0, "open(slave) failed");
        // The master must NOT leak into the spawned shell: an inherited
        // master copy would keep the pty alive after the parent closes
        // its own, and no EOF would ever reach the slave. (Command's
        // stdio dup2 clears CLOEXEC on the slave copies it installs.)
        libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(slave, libc::F_SETFD, libc::FD_CLOEXEC);
        (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave))
    }
}

#[test]
fn shell_exits_when_pty_master_closes() {
    let (master, slave) = open_pty();
    let tmpdir = TempDir::new();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yosh"));
    cmd.env("TERM", "dumb")
        .env("HOME", tmpdir.path())
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave));
    // Make the slave the child's controlling terminal so the setup
    // matches a real terminal emulator (and expectrl's).
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("failed to spawn yosh");

    // Wait until the shell prints its PS1 prompt, so the master closes
    // while yosh sits in the interactive read loop (the buggy state).
    {
        // SAFETY: master is a valid owned fd; F_SETFL to nonblocking.
        unsafe {
            let fl = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, fl | libc::O_NONBLOCK);
        }
        let mut file = std::fs::File::from(master.try_clone().unwrap());
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut buf = [0u8; 4096];
        while Instant::now() < deadline {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => seen.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
            if seen.windows(2).any(|w| w == b"$ ") {
                break;
            }
        }
        assert!(
            seen.windows(2).any(|w| w == b"$ "),
            "prompt never appeared; got: {:?}",
            String::from_utf8_lossy(&seen)
        );
    }

    // Close the master: the terminal is gone.
    drop(master);

    // The shell must exit on its own — generous deadline for CI load.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().expect("try_wait failed") {
            Some(status) => {
                // EOF exit path: same as Ctrl+D, normal termination.
                assert!(
                    status.success(),
                    "shell exited with non-success status: {:?}",
                    status
                );
                break;
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("shell did not exit within 10s after pty master closed");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
