use std::os::fd::RawFd;

use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;

use crate::env::ShellEnv;
use crate::expand::expand_word_to_string;
use crate::parser::ast::{Redirect, RedirectKind};

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
pub struct RedirectState {
    /// (original_fd, saved_copy_fd) — used for restore
    saved_fds: Vec<(RawFd, RawFd)>,
}

impl RedirectState {
    pub fn new() -> Self {
        RedirectState {
            saved_fds: Vec::new(),
        }
    }

    /// Apply a list of redirects.
    /// If `save` is true (builtin case), save the original fds so they can be restored.
    pub fn apply(
        &mut self,
        redirects: &[Redirect],
        env: &ShellEnv,
        save: bool,
    ) -> Result<(), String> {
        for redirect in redirects {
            self.apply_one(redirect, env, save)?;
        }
        Ok(())
    }

    fn apply_one(&mut self, redirect: &Redirect, env: &ShellEnv, save: bool) -> Result<(), String> {
        match &redirect.kind {
            RedirectKind::Input(word) => {
                let target_fd = redirect.fd.unwrap_or(0);
                let path = expand_word_to_string(env, word);
                let fd = open(path.as_str(), OFlag::O_RDONLY, Mode::empty())
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                unsafe { libc::close(fd) };
            }
            RedirectKind::Output(word) | RedirectKind::OutputClobber(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let path = expand_word_to_string(env, word);
                let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
                let fd = open(path.as_str(), flags, Mode::from_bits_truncate(0o644))
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                unsafe { libc::close(fd) };
            }
            RedirectKind::Append(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let path = expand_word_to_string(env, word);
                let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_APPEND;
                let fd = open(path.as_str(), flags, Mode::from_bits_truncate(0o644))
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                unsafe { libc::close(fd) };
            }
            RedirectKind::DupOutput(word) => {
                let target_fd = redirect.fd.unwrap_or(1);
                let src = expand_word_to_string(env, word);
                if src == "-" {
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    unsafe { libc::close(target_fd) };
                } else {
                    let src_fd: RawFd = src
                        .parse()
                        .map_err(|_| format!("{}: invalid file descriptor", src))?;
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    raw_dup2(src_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                }
            }
            RedirectKind::DupInput(word) => {
                let target_fd = redirect.fd.unwrap_or(0);
                let src = expand_word_to_string(env, word);
                if src == "-" {
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    unsafe { libc::close(target_fd) };
                } else {
                    let src_fd: RawFd = src
                        .parse()
                        .map_err(|_| format!("{}: invalid file descriptor", src))?;
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    raw_dup2(src_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                }
            }
            RedirectKind::ReadWrite(word) => {
                let target_fd = redirect.fd.unwrap_or(0);
                let path = expand_word_to_string(env, word);
                let flags = OFlag::O_RDWR | OFlag::O_CREAT;
                let fd = open(path.as_str(), flags, Mode::from_bits_truncate(0o644))
                    .map_err(|e| format!("{}: {}", path, e))?
                    .into_raw_fd();
                if save {
                    self.save_fd(target_fd)?;
                }
                raw_dup2(fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                unsafe { libc::close(fd) };
            }
            RedirectKind::HereDoc(_) => {
                // Phase 4 — skip for now
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
        ShellEnv::new("kish", vec![])
    }

    #[test]
    fn test_redirect_output_and_restore() {
        let env = make_env();
        let tmp = std::env::temp_dir().join("kish_redirect_test_output.txt");
        let path_str = tmp.to_str().unwrap().to_string();

        let redirects = vec![Redirect {
            fd: Some(1),
            kind: RedirectKind::Output(Word::literal(&path_str)),
        }];

        let mut state = RedirectState::new();
        state.apply(&redirects, &env, true).expect("apply should succeed");

        // Write to stdout (fd 1), which is now the file
        use std::io::Write;
        use std::os::unix::io::FromRawFd;
        let mut stdout = unsafe { std::fs::File::from_raw_fd(1) };
        write!(stdout, "hello redirect").unwrap();
        // Don't close fd 1 — just flush and forget the File wrapper
        std::mem::forget(stdout);

        state.restore();

        let contents = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(contents.contains("hello redirect"), "file should contain written text");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_redirect_input() {
        use std::io::Read;
        use std::os::unix::io::FromRawFd;

        let env = make_env();
        let tmp = std::env::temp_dir().join("kish_redirect_test_input.txt");
        std::fs::write(&tmp, "test input\n").unwrap();
        let path_str = tmp.to_str().unwrap().to_string();

        let redirects = vec![Redirect {
            fd: Some(0),
            kind: RedirectKind::Input(Word::literal(&path_str)),
        }];

        let mut state = RedirectState::new();
        state.apply(&redirects, &env, true).expect("apply should succeed");

        let mut buf = String::new();
        let mut stdin = unsafe { std::fs::File::from_raw_fd(0) };
        stdin.read_to_string(&mut buf).ok();
        std::mem::forget(stdin);

        state.restore();

        assert!(buf.contains("test input"));
        let _ = std::fs::remove_file(&tmp);
    }
}
