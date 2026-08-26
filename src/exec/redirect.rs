use std::os::fd::RawFd;

use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;

use crate::env::ShellEnv;
use crate::expand::expand_word_to_string;
use crate::parser::ast::{Redirect, RedirectKind};

/// Open a byteenc-encoded redirect target, decoding escape codepoints so
/// non-UTF-8 paths reach the OS as their original raw bytes.
fn open_decoded(path: &str, flags: OFlag, mode: Mode) -> nix::Result<std::os::fd::OwnedFd> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = crate::byteenc::decode_bytes(path);
    open(std::ffi::OsStr::from_bytes(&bytes), flags, mode)
}

/// Open `path` for `>` output under `set -C` (noclobber) without the
/// TOCTOU race of a separate exists() check: O_CREAT|O_EXCL atomically
/// fails with EEXIST if anything (including a symlink) already occupies
/// the path. Existing *non-regular* files (e.g. /dev/null, FIFOs) are
/// then reopened without O_CREAT/O_TRUNC, per POSIX — noclobber only
/// protects regular files.
fn open_noclobber(path: &str) -> Result<RawFd, String> {
    let create_flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL;
    match open_decoded(path, create_flags, Mode::from_bits_truncate(0o644)) {
        Ok(fd) => Ok(fd.into_raw_fd()),
        Err(nix::errno::Errno::EEXIST) => {
            // stat() follows symlinks: a dangling link errors here, and a
            // link to a regular file is treated as that regular file.
            match std::fs::metadata({
                use std::os::unix::ffi::OsStrExt;
                std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
                    &crate::byteenc::decode_bytes(path),
                ))
            }) {
                Ok(md) if !md.is_file() => open_decoded(path, OFlag::O_WRONLY, Mode::empty())
                    .map(|fd| fd.into_raw_fd())
                    .map_err(|e| format!("{}: {}", path, e)),
                _ => Err(format!("{}: cannot overwrite existing file", path)),
            }
        }
        Err(e) => Err(format!("{}: {}", path, e)),
    }
}

/// Heredoc bodies at or below this size are delivered via an anonymous
/// pipe, written in full before any reader is attached. POSIX only
/// guarantees a pipe absorbs PIPE_BUF (>= 512) bytes without a reader;
/// real capacities are 16-64 KiB. 4096 stays conservatively below any
/// real capacity so the up-front write cannot block. Larger bodies are
/// spooled through an unlinked temp file instead.
const HEREDOC_PIPE_MAX: usize = 4096;

/// Produce a readable fd that yields `body`, without blocking the shell
/// regardless of body size.
fn heredoc_source_fd(body: &[u8]) -> Result<RawFd, String> {
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::io::FromRawFd;

    if body.len() <= HEREDOC_PIPE_MAX {
        let mut fds: [RawFd; 2] = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
            return Err(format!("pipe: {}", std::io::Error::last_os_error()));
        }
        let (read_fd, write_fd) = (fds[0], fds[1]);
        {
            let mut write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
            let _ = write_file.write_all(body);
            // drop closes write_fd
        }
        return Ok(read_fd);
    }

    // Spool via an unlinked temp file (what real shells do): the fd
    // stays valid, the name disappears immediately.
    let template = std::env::temp_dir().join("yosh-heredoc.XXXXXX");
    let mut template_bytes = {
        use std::os::unix::ffi::OsStrExt;
        template.as_os_str().as_bytes().to_vec()
    };
    template_bytes.push(0);
    let fd = unsafe { libc::mkstemp(template_bytes.as_mut_ptr() as *mut libc::c_char) };
    if fd == -1 {
        return Err(format!(
            "heredoc: mkstemp: {}",
            std::io::Error::last_os_error()
        ));
    }
    unsafe { libc::unlink(template_bytes.as_ptr() as *const libc::c_char) };

    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.write_all(body)
        .map_err(|e| format!("heredoc: write: {}", e))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("heredoc: seek: {}", e))?;
    Ok(file.into_raw_fd())
}

/// Perform a low-level dup2(oldfd, newfd) via libc.
fn raw_dup2(oldfd: RawFd, newfd: RawFd) -> nix::Result<()> {
    let res = unsafe { libc::dup2(oldfd, newfd) };
    if res == -1 {
        Err(nix::errno::Errno::last())
    } else {
        Ok(())
    }
}

/// Tracks saved file descriptors so they can be restored after a builtin runs.
#[derive(Default)]
pub struct RedirectState {
    /// (original_fd, saved_copy_fd) — used for restore
    saved_fds: Vec<(RawFd, RawFd)>,
}

impl RedirectState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a list of redirects.
    /// If `save` is true (builtin case), save the original fds so they can be restored.
    ///
    /// On failure, any redirects already applied within this call are rolled back
    /// (via `self.restore()`), so the returned `Err` always reports a state where
    /// the caller's fd table is unchanged. `save=false` leaves `saved_fds` empty,
    /// so the rollback is a no-op in that case — meaning fds opened by successful
    /// redirects preceding the failing one are leaked (out of scope per spec §C;
    /// callers in this mode are forked children that exit on failure anyway).
    pub fn apply(
        &mut self,
        redirects: &[Redirect],
        env: &mut ShellEnv,
        save: bool,
    ) -> Result<(), String> {
        for redirect in redirects {
            if let Err(e) = self.apply_one(redirect, env, save) {
                self.restore();
                return Err(e);
            }
        }
        Ok(())
    }

    fn apply_one(
        &mut self,
        redirect: &Redirect,
        env: &mut ShellEnv,
        save: bool,
    ) -> Result<(), String> {
        match &redirect.kind {
            RedirectKind::Input(word) => {
                let target_fd = redirect.fd.unwrap_or(0);
                let path = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                let fd = open_decoded(&path, OFlag::O_RDONLY, Mode::empty())
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                if fd != target_fd {
                    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    unsafe { libc::close(fd) };
                }
            }
            RedirectKind::Output(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let path = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                let fd = if env.mode.options.noclobber {
                    open_noclobber(&path)?
                } else {
                    let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
                    open_decoded(&path, flags, Mode::from_bits_truncate(0o644))
                        .map_err(|e| format!("{}: {}", path, e))?
                        .into_raw_fd()
                };
                if save {
                    self.save_fd(target_fd)?;
                }
                if fd != target_fd {
                    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    unsafe { libc::close(fd) };
                }
            }
            RedirectKind::OutputClobber(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let path = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
                let fd = open_decoded(&path, flags, Mode::from_bits_truncate(0o644))
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                if fd != target_fd {
                    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    unsafe { libc::close(fd) };
                }
            }
            RedirectKind::Append(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let path = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_APPEND;
                let fd = open_decoded(&path, flags, Mode::from_bits_truncate(0o644))
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                if fd != target_fd {
                    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    unsafe { libc::close(fd) };
                }
            }
            RedirectKind::DupOutput(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let src = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                if src == "-" {
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    unsafe { libc::close(target_fd) };
                } else {
                    let src_fd: RawFd = src
                        .parse()
                        .map_err(|_| format!("{}: invalid file descriptor", src))?;
                    if src_fd != target_fd {
                        if save {
                            self.save_fd(target_fd)?;
                        }
                        raw_dup2(src_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    }
                }
            }
            RedirectKind::DupInput(word) => {
                let target_fd = redirect.fd.unwrap_or(0);
                let src = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                if src == "-" {
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    unsafe { libc::close(target_fd) };
                } else {
                    let src_fd: RawFd = src
                        .parse()
                        .map_err(|_| format!("{}: invalid file descriptor", src))?;
                    if src_fd != target_fd {
                        if save {
                            self.save_fd(target_fd)?;
                        }
                        raw_dup2(src_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    }
                }
            }
            RedirectKind::ReadWrite(word) => {
                let target_fd = redirect.fd.unwrap_or(0);
                let path = expand_word_to_string(env, word).map_err(|e| e.to_string())?;
                let flags = OFlag::O_RDWR | OFlag::O_CREAT;
                let fd = open_decoded(&path, flags, Mode::from_bits_truncate(0o644))
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                if fd != target_fd {
                    raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    unsafe { libc::close(fd) };
                }
            }
            RedirectKind::HereDoc(heredoc) => {
                let target_fd = redirect.fd.unwrap_or(0);

                // Expand the body. An expansion failure (e.g. $((1/0)))
                // becomes a redirection error: the command is aborted but
                // the shell continues — dash and bash both keep the shell
                // alive for a heredoc arithmetic failure, unlike the word
                // path (empirical 2026-08-26; POSIX §2.7.4 heredocs are
                // redirections, so the §2.8.1 redirection-error row
                // applies, not the expansion-error row).
                let body = crate::expand::expand_heredoc_body(env, &heredoc.body, heredoc.quoted)
                    .map_err(|e| e.message)?;

                // Braced-parameter failures (`${x:?msg}`, `set -u` unset)
                // report through `param::expansion_error` instead of `Err`:
                // the diagnostic is already printed and
                // `FlowControl::ExpansionError` is raised. Convert that to
                // the same redirection-error outcome as above (command
                // skipped, status 1, shell continues — matching dash;
                // empirical 2026-08-26): clear the flow control so it does
                // not abort the non-interactive shell, and return the
                // already-reported sentinel (empty message) so
                // `report_command_error` does not print a second line.
                if env.exec.flow_control == Some(crate::env::FlowControl::ExpansionError) {
                    env.exec.flow_control = None;
                    return Err(String::new());
                }

                let read_fd = heredoc_source_fd(&crate::byteenc::decode_bytes(&body))?;

                // Connect read end to target fd (stdin by default)
                if save {
                    self.save_fd(target_fd)?;
                }
                if read_fd != target_fd {
                    raw_dup2(read_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                    unsafe { libc::close(read_fd) };
                }
            }
        }
        Ok(())
    }

    /// dup() the fd before it gets overwritten, storing the saved copy.
    /// Uses F_DUPFD_CLOEXEC so the saved fd is automatically closed in child processes.
    fn save_fd(&mut self, fd: RawFd) -> Result<(), String> {
        let saved = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        if saved == -1 {
            return Err(format!("dup: {}", std::io::Error::last_os_error()));
        }
        self.saved_fds.push((fd, saved));
        Ok(())
    }

    /// Restore all saved fds in reverse order (LIFO).
    pub fn restore(&mut self) {
        for (original, saved) in self.saved_fds.drain(..).rev() {
            raw_dup2(saved, original).ok();
            unsafe { libc::close(saved) };
        }
    }
}

impl Drop for RedirectState {
    fn drop(&mut self) {
        // Close any saved fds that were never restored
        for (_original, saved) in self.saved_fds.drain(..) {
            unsafe { libc::close(saved) };
        }
    }
}

// Bring IntoRawFd into scope for the `.into_raw_fd()` calls on OwnedFd
use std::os::unix::io::IntoRawFd;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{Redirect, RedirectKind, Word};

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    /// Tests that manipulate process-wide file descriptors (fd 0, fd 1, fd 2) must
    /// hold this lock for their entire duration so that cargo test's parallel threads
    /// do not corrupt each other's fd table.
    static FD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_redirect_output_and_restore() {
        let _guard = FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut env = make_env();
        let tmp = std::env::temp_dir().join("yosh_redirect_test_output.txt");
        let path_str = tmp.to_str().unwrap().to_string();

        let redirects = vec![Redirect {
            fd: Some(1),
            kind: RedirectKind::Output(Word::literal(&path_str)),
        }];

        let mut state = RedirectState::new();
        state
            .apply(&redirects, &mut env, true)
            .expect("apply should succeed");

        // Write to stdout (fd 1), which is now the file
        use std::io::Write;
        use std::os::unix::io::FromRawFd;
        let mut stdout = unsafe { std::fs::File::from_raw_fd(1) };
        write!(stdout, "hello redirect").unwrap();
        // Don't close fd 1 — just flush and forget the File wrapper
        std::mem::forget(stdout);

        state.restore();

        let contents = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(
            contents.contains("hello redirect"),
            "file should contain written text"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_redirect_input() {
        let _guard = FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Read;
        use std::os::unix::io::FromRawFd;

        let mut env = make_env();
        let tmp = std::env::temp_dir().join("yosh_redirect_test_input.txt");
        std::fs::write(&tmp, "test input\n").unwrap();
        let path_str = tmp.to_str().unwrap().to_string();

        let redirects = vec![Redirect {
            fd: Some(0),
            kind: RedirectKind::Input(Word::literal(&path_str)),
        }];

        let mut state = RedirectState::new();
        state
            .apply(&redirects, &mut env, true)
            .expect("apply should succeed");

        let mut buf = String::new();
        let mut stdin = unsafe { std::fs::File::from_raw_fd(0) };
        stdin.read_to_string(&mut buf).ok();
        std::mem::forget(stdin);

        state.restore();

        assert!(buf.contains("test input"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_apply_rolls_back_on_second_redirect_failure() {
        let _guard = FD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Two redirects: first targets a valid tmp file (fd 1), second targets
        // a path whose parent directory does not exist (fd 2) so open() fails.
        // Pre-fix: saved_fds is non-empty after Err and fd 1 remains dup2'd over
        // the tmp file, so a subsequent libc::write(1, ...) leaks into the tmp file.
        // Post-fix: apply() calls self.restore() internally, saved_fds is empty,
        // and fd 1 points back at the pre-apply target.

        let mut env = make_env();
        let tmp_ok = std::env::temp_dir().join("yosh_apply_rollback_ok.txt");
        // Remove any stale file from a prior test run.
        let _ = std::fs::remove_file(&tmp_ok);
        let bad_path = "/no/such/dir/should-not-exist-yosh-test/file.txt";

        let redirects = vec![
            Redirect {
                fd: Some(1),
                kind: RedirectKind::Output(Word::literal(tmp_ok.to_str().unwrap())),
            },
            Redirect {
                fd: Some(2),
                kind: RedirectKind::Output(Word::literal(bad_path)),
            },
        ];

        // Save original fd 1 outside RedirectState so we can restore it at the end
        // (cargo test captures stdout; we must not leave fd 1 corrupted for sibling tests).
        let orig_stdout = unsafe { libc::dup(1) };
        assert!(orig_stdout >= 0, "dup(1) failed");

        let mut state = RedirectState::new();
        let result = state.apply(&redirects, &mut env, true);
        assert!(result.is_err(), "expected apply to fail on the bad path");

        // Post-condition 1: rollback emptied saved_fds.
        assert!(
            state.saved_fds.is_empty(),
            "saved_fds should be empty after rollback, got {} entries",
            state.saved_fds.len()
        );

        // Post-condition 2: writes to fd 1 should not land in tmp_ok.
        let marker = b"post-rollback-marker\n";
        unsafe {
            libc::write(1, marker.as_ptr() as *const _, marker.len());
        }

        let written = std::fs::read_to_string(&tmp_ok).unwrap_or_default();

        // Cleanup BEFORE assertion so a failure still cleans up.
        unsafe {
            libc::dup2(orig_stdout, 1);
            libc::close(orig_stdout);
        }
        let _ = std::fs::remove_file(&tmp_ok);

        assert!(
            !written.contains("post-rollback-marker"),
            "fd 1 should not still point at tmp_ok after rollback; tmp_ok contained: {written:?}"
        );
    }
}
