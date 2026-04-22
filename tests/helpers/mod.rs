pub mod mock_terminal;

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Async-signal-safe helper used inside `pre_exec`. Direct `sigaction(2)`
/// call with no allocation on the success path.
unsafe fn reset_to_default(sig: libc::c_int) -> std::io::Result<()> {
    let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
    sa.sa_sigaction = libc::SIG_DFL;
    let rc = unsafe { libc::sigaction(sig, &sa, std::ptr::null_mut()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Reset SIGINT and SIGQUIT to `SIG_DFL` in the child before `exec`.
///
/// POSIX §2.11 requires a shell without job control to inherit asynchronous
/// commands with SIGINT/SIGQUIT set to `SIG_IGN`. When the test binary itself
/// is launched in that role — e.g. backgrounded by the invoking shell, or by
/// some `cargo test` jobserver configurations — the child yosh would observe
/// those signals as SIG_IGN at startup and capture them as "ignored on entry",
/// silently no-op'ing every `trap` that targets them. Without this reset,
/// trap-related tests in `signals.rs` and `subshell.rs` flake in that
/// environment while passing in isolation.
///
/// Every test that spawns yosh via `Command::new` and may be run under cargo
/// test's parallel scheduler should call this on the Command before `output()`
/// / `spawn()`. `tests/ignored_on_entry.rs` deliberately opts out — it needs
/// SIG_IGN inherited to exercise yosh's POSIX §2.11 capture path.
pub fn reset_trap_signals(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| unsafe {
            reset_to_default(libc::SIGINT)?;
            reset_to_default(libc::SIGQUIT)?;
            Ok(())
        });
    }
}

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("yosh-test-{}-{}", id, seq));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write_file(&self, name: &str, content: &str) -> PathBuf {
        let file_path = self.path.join(name);
        std::fs::write(&file_path, content).unwrap();
        file_path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
