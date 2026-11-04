//! Applying expanded redirections to the process fd table, one kind at
//! a time.

use std::os::fd::RawFd;

use nix::fcntl::OFlag;
use nix::sys::stat::Mode;

use super::RedirectState;
use super::expand::{ExpandedOp, ExpandedRedirect, expand_redirects};
use super::heredoc::heredoc_source_fd;
use super::open::{CREATE_MODE, errno_text, open_noclobber, open_path, raw_dup2};
use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::parser::ast::Redirect;

impl RedirectState {
    /// Expand and apply a list of redirects (see [`expand_redirects`] and
    /// [`Self::apply_expanded`]).
    pub fn apply(
        &mut self,
        redirects: &[Redirect],
        env: &mut ShellEnv,
        save: bool,
    ) -> Result<(), ShellError> {
        let expanded = expand_redirects(redirects, env)?;
        self.apply_expanded(&expanded, save)
    }

    /// Apply already-expanded redirects.
    /// If `save` is true (builtin case), save the original fds so they can be restored.
    ///
    /// On failure, any redirects already applied within this call are rolled back
    /// (via `self.restore()`), so the returned `Err` always reports a state where
    /// the caller's fd table is unchanged. `save=false` leaves `saved_fds` empty,
    /// so the rollback is a no-op in that case — meaning fds opened by successful
    /// redirects preceding the failing one are leaked (out of scope per spec §C;
    /// callers in this mode are forked children that exit on failure anyway).
    /// Errors are `RedirectFailed` runtime errors.
    pub fn apply_expanded(
        &mut self,
        redirects: &[ExpandedRedirect],
        save: bool,
    ) -> Result<(), ShellError> {
        for redirect in redirects {
            if let Err(e) = self.apply_one(redirect, save) {
                self.restore();
                return Err(ShellError::runtime(RuntimeErrorKind::RedirectFailed, e));
            }
        }
        Ok(())
    }

    fn apply_one(&mut self, redirect: &ExpandedRedirect, save: bool) -> Result<(), String> {
        let target_fd = redirect.fd;
        match &redirect.op {
            ExpandedOp::Input(path) => self.install(target_fd, save, || {
                open_path(path, OFlag::O_RDONLY, Mode::empty())
            }),
            ExpandedOp::Output { path, noclobber } => self.install(target_fd, save, || {
                if *noclobber {
                    open_noclobber(path)
                } else {
                    let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
                    open_path(path, flags, CREATE_MODE)
                }
            }),
            ExpandedOp::Append(path) => self.install(target_fd, save, || {
                let flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_APPEND;
                open_path(path, flags, CREATE_MODE)
            }),
            ExpandedOp::ReadWrite(path) => self.install(target_fd, save, || {
                open_path(path, OFlag::O_RDWR | OFlag::O_CREAT, CREATE_MODE)
            }),
            ExpandedOp::Dup(src) => self.duplicate(target_fd, src, save),
            ExpandedOp::HereDoc(body) => self.install(target_fd, save, || heredoc_source_fd(body)),
        }
    }

    /// `>&src` / `<&src`: close `target_fd` when `src` is `-`, otherwise
    /// make it a copy of fd `src` (a no-op when both are the same fd).
    fn duplicate(&mut self, target_fd: RawFd, src: &str, save: bool) -> Result<(), String> {
        if src == "-" {
            self.save_target(target_fd, save)?;
            unsafe { libc::close(target_fd) };
            return Ok(());
        }
        let src_fd: RawFd = src
            .parse()
            .map_err(|_| format!("{}: invalid file descriptor", src))?;
        if src_fd != target_fd {
            self.save_target(target_fd, save)?;
            raw_dup2(src_fd, target_fd).map_err(|e| format!("{}: {}", src, errno_text(e)))?;
        }
        Ok(())
    }

    /// Point `target_fd` at the fd produced by `open`. The target is saved
    /// *before* opening: if it is currently closed, `open` may return the
    /// target number itself, and saving afterwards would record the new
    /// file as the "original" and leak it past `restore`.
    fn install(
        &mut self,
        target_fd: RawFd,
        save: bool,
        open: impl FnOnce() -> Result<RawFd, String>,
    ) -> Result<(), String> {
        self.save_target(target_fd, save)?;
        let fd = open()?;
        if fd != target_fd {
            let res = raw_dup2(fd, target_fd);
            unsafe { libc::close(fd) };
            res.map_err(|e| format!("dup2: {}", errno_text(e)))?;
        }
        Ok(())
    }

    /// [`Self::save_fd`] when the caller asked for the redirection to be
    /// undone later (`save`); a no-op otherwise.
    fn save_target(&mut self, fd: RawFd, save: bool) -> Result<(), String> {
        if save { self.save_fd(fd) } else { Ok(()) }
    }
}
