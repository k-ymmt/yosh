use std::os::fd::RawFd;
use std::os::unix::io::IntoRawFd;

use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;

use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::expand::expand_word_to_string;
use crate::parser::ast::{Redirect, RedirectKind, Word};

/// Open a byteenc-encoded redirect target, decoding escape codepoints so
/// non-UTF-8 paths reach the OS as their original raw bytes.
fn open_decoded(path: &str, flags: OFlag, mode: Mode) -> nix::Result<std::os::fd::OwnedFd> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = crate::byteenc::decode_bytes(path);
    open(std::ffi::OsStr::from_bytes(&bytes), flags, mode)
}

/// The C library's message for `e` (`No such file or directory`), for
/// redirection diagnostics: nix's own `Display` adds the symbolic name
/// (`ENOENT: ...`) and has its own wording for some codes.
pub(crate) fn errno_text(e: nix::errno::Errno) -> String {
    io_error_text(&std::io::Error::from_raw_os_error(e as i32))
}

/// An I/O error as the shell reports it: the C library's message alone
/// (`No such file or directory`), without the ` (os error 2)` suffix
/// Rust's `Display` appends for OS errors.
pub(crate) fn io_error_text(e: &std::io::Error) -> String {
    let msg = e.to_string();
    match (e.raw_os_error(), msg.rfind(" (os error ")) {
        (Some(_), Some(pos)) => msg[..pos].to_string(),
        _ => msg,
    }
}

/// [`open_decoded`] as a raw fd, with the error formatted as the
/// redirection diagnostic (`path: reason`).
fn open_path(path: &str, flags: OFlag, mode: Mode) -> Result<RawFd, String> {
    open_decoded(path, flags, mode)
        .map(|fd| fd.into_raw_fd())
        .map_err(|e| format!("{}: {}", path, errno_text(e)))
}

/// Mode for files created by `>`, `>>` and `<>`.
const CREATE_MODE: Mode = Mode::from_bits_truncate(0o644);

/// Open `path` for `>` output under `set -C` (noclobber) without the
/// TOCTOU race of a separate exists() check: O_CREAT|O_EXCL atomically
/// fails with EEXIST if anything (including a symlink) already occupies
/// the path. Existing *non-regular* files (e.g. /dev/null, FIFOs) are
/// then reopened without O_CREAT/O_TRUNC, per POSIX — noclobber only
/// protects regular files.
fn open_noclobber(path: &str) -> Result<RawFd, String> {
    let create_flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL;
    match open_decoded(path, create_flags, CREATE_MODE) {
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
                Ok(md) if !md.is_file() => open_path(path, OFlag::O_WRONLY, Mode::empty()),
                _ => Err(format!("{}: cannot overwrite existing file", path)),
            }
        }
        Err(e) => Err(format!("{}: {}", path, errno_text(e))),
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

/// What a redirection does once its word has been expanded.
#[derive(Debug, Clone, PartialEq)]
enum ExpandedOp {
    /// `<path`
    Input(String),
    /// `>path` (`noclobber` captured at expansion time) and `>|path`
    Output { path: String, noclobber: bool },
    /// `>>path`
    Append(String),
    /// `<>path`
    ReadWrite(String),
    /// `>&word` / `<&word` — `-` closes, otherwise a source fd number.
    Dup(String),
    /// Here-document body, already expanded and decoded to raw bytes.
    HereDoc(Vec<u8>),
}

/// A redirection whose word (or here-document body) has already been
/// expanded in the shell process. Splitting expansion from application
/// lets the shell expand every redirection word *before* the command's
/// prefix assignments (POSIX §2.9.1 step order) and before forking an
/// external utility, so an expansion error in a redirection word is a
/// shell-level expansion error (POSIX §2.8.1: a non-interactive shell
/// exits — dash agrees) rather than a failure confined to the forked
/// child.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedRedirect {
    fd: RawFd,
    op: ExpandedOp,
}

/// Expand the words of `redirects` in order.
///
/// Error contract:
/// - A word expansion failure is returned unchanged (kind `Expansion`
///   for arithmetic errors, which the caller reports and which makes a
///   non-interactive shell exit).
/// - A braced-parameter failure in a word (`${x:?}`, `set -u`) has
///   already been diagnosed and left `FlowControl::ExpansionError` set;
///   it is returned as [`ShellError::expansion_already_reported`] and the
///   flow control stays raised so the shell exits / aborts the line.
/// - A here-document body failure stays a *redirection* error (command
///   skipped, status 1, the shell continues — dash and bash agree,
///   empirical 2026-08-26; POSIX §2.7.4 heredocs are redirections, so the
///   §2.8.1 redirection-error row applies).
pub fn expand_redirects(
    redirects: &[Redirect],
    env: &mut ShellEnv,
) -> Result<Vec<ExpandedRedirect>, ShellError> {
    let mut out = Vec::with_capacity(redirects.len());
    for redirect in redirects {
        out.push(expand_one(redirect, env)?);
    }
    Ok(out)
}

fn expansion_failed(env: &ShellEnv) -> bool {
    env.exec.flow_control == Some(crate::env::FlowControl::ExpansionError)
}

fn expand_redirect_word(env: &mut ShellEnv, word: &Word) -> Result<String, ShellError> {
    let s = expand_word_to_string(env, word)?;
    if expansion_failed(env) {
        // Diagnostic already printed by `param::expansion_error`.
        return Err(ShellError::expansion_already_reported());
    }
    Ok(s)
}

fn expand_one(redirect: &Redirect, env: &mut ShellEnv) -> Result<ExpandedRedirect, ShellError> {
    let (default_fd, op) = match &redirect.kind {
        RedirectKind::Input(w) => (0, ExpandedOp::Input(expand_redirect_word(env, w)?)),
        RedirectKind::Output(w) => (
            1,
            ExpandedOp::Output {
                path: expand_redirect_word(env, w)?,
                noclobber: env.mode.options.noclobber,
            },
        ),
        RedirectKind::OutputClobber(w) => (
            1,
            ExpandedOp::Output {
                path: expand_redirect_word(env, w)?,
                noclobber: false,
            },
        ),
        RedirectKind::Append(w) => (1, ExpandedOp::Append(expand_redirect_word(env, w)?)),
        RedirectKind::ReadWrite(w) => (0, ExpandedOp::ReadWrite(expand_redirect_word(env, w)?)),
        RedirectKind::DupOutput(w) => (1, ExpandedOp::Dup(expand_redirect_word(env, w)?)),
        RedirectKind::DupInput(w) => (0, ExpandedOp::Dup(expand_redirect_word(env, w)?)),
        RedirectKind::HereDoc(heredoc) => {
            // See the error contract on `expand_redirects`: heredoc body
            // failures are redirection errors, not fatal expansion errors.
            let body = crate::expand::expand_heredoc_body(env, &heredoc.body, heredoc.quoted)
                .map_err(|e| ShellError::runtime(RuntimeErrorKind::RedirectFailed, e.message))?;
            if expansion_failed(env) {
                // `${x:?}` / nounset inside the body: already reported.
                // Clear the flow control so it does not abort the shell,
                // and return the already-reported sentinel (empty
                // message) so `report_command_error` stays silent.
                env.exec.flow_control = None;
                return Err(ShellError::runtime(
                    RuntimeErrorKind::RedirectFailed,
                    String::new(),
                ));
            }
            (
                0,
                ExpandedOp::HereDoc(crate::byteenc::decode_bytes(&body).into_owned()),
            )
        }
    };
    Ok(ExpandedRedirect {
        fd: redirect.fd.unwrap_or(default_fd),
        op,
    })
}

/// Sentinel stored in `saved_fds` when the target fd was closed before
/// the redirection: restoring it means closing the fd again.
const WAS_CLOSED: RawFd = -1;

/// One fd a [`RedirectState`] overwrote: `(original_fd, saved_copy_fd,
/// original_was_cloexec)`. `saved_copy_fd` is [`WAS_CLOSED`] when
/// `original_fd` was not open.
type SavedFd = (RawFd, RawFd, bool);

thread_local! {
    /// The saved fds of every [`RedirectState`] currently applied in this
    /// process, innermost last. Redirection scopes nest strictly (a
    /// compound command's redirections are restored after its body's),
    /// so each state owns the suffix of this stack that it pushed.
    ///
    /// Keeping them in one stack lets a shell that is about to terminate
    /// undo every active redirection before it runs the EXIT trap
    /// ([`restore_all_saved_redirections`]): `trap 'echo out' EXIT; {
    /// exit; } >f` must print `out` on the shell's own stdout, not into
    /// `f` (dash and bash unwind the redirection first).
    static SAVED_FDS: std::cell::RefCell<Vec<SavedFd>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Bumped whenever the whole stack is emptied out of band
    /// ([`restore_all_saved_redirections`], a fork): a [`RedirectState`]
    /// from an older epoch no longer owns any entry, and must not pop
    /// those of states created after the reset.
    static SAVED_FDS_EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn saved_fds_epoch() -> u64 {
    SAVED_FDS_EPOCH.with(|e| e.get())
}

fn bump_saved_fds_epoch() {
    SAVED_FDS_EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
}

fn saved_fds_len() -> usize {
    SAVED_FDS.with(|s| s.borrow().len())
}

/// Pop the saved fds above `base` (innermost first), restoring each one
/// to its original fd.
fn unwind_saved_fds(base: usize) {
    while let Some((original, saved, cloexec)) = SAVED_FDS.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() > base { s.pop() } else { None }
    }) {
        if saved == WAS_CLOSED {
            unsafe { libc::close(original) };
        } else {
            if raw_dup2(saved, original).is_ok() && cloexec {
                unsafe { libc::fcntl(original, libc::F_SETFD, libc::FD_CLOEXEC) };
            }
            unsafe { libc::close(saved) };
        }
    }
}

/// Undo every redirection currently applied with `save` in this
/// process, innermost first. Called when the shell (or a subshell) is
/// terminating, right before its EXIT trap runs, so the action writes to
/// the shell's own fds — the redirection scopes it is exiting from are
/// never re-entered. A [`RedirectState`] whose fds were undone this way
/// finds nothing left to restore.
pub(crate) fn restore_all_saved_redirections() {
    unwind_saved_fds(0);
    bump_saved_fds_epoch();
}

/// Forget (without restoring) the saved fds inherited from the parent
/// shell: called first thing in every forked child. The redirections the
/// parent applied before forking are part of the child's environment
/// (`( trap 'echo t' EXIT; exit ) >f` writes `t` into `f`), so the
/// child's exit must not undo them. The saved copies are close-on-exec
/// and stay open until the child exits.
pub(crate) fn forget_inherited_saved_redirections() {
    SAVED_FDS.with(|s| s.borrow_mut().clear());
    bump_saved_fds_epoch();
}

/// Tracks saved file descriptors so they can be restored after a builtin
/// or compound command runs. The saved fds live on the per-process
/// `SAVED_FDS` stack; once this state saves its first fd it owns the
/// entries above `base` while the stack is still in the `epoch` it was
/// in at that point.
///
/// The ownership is claimed lazily, on the first save: most commands
/// have no redirections at all, and a state that never saved touches
/// neither thread-local on creation, restore or drop (the eager claim
/// cost ~3% of a function-call loop). Scopes nest strictly, so the stack
/// length at the first save equals the length at creation.
pub struct RedirectState {
    /// `(base, epoch)` once this state has saved an fd.
    owned: Option<(usize, u64)>,
}

impl Default for RedirectState {
    fn default() -> Self {
        Self::new()
    }
}

impl RedirectState {
    pub fn new() -> Self {
        Self { owned: None }
    }

    /// Claim the stack suffix above its current top, before the first
    /// push (see the type docs).
    fn claim(&mut self) {
        if self.owned.is_none() {
            self.owned = Some((saved_fds_len(), saved_fds_epoch()));
        }
    }

    /// Restore this state's saved fds, unless an out-of-band reset
    /// already did (see `SAVED_FDS_EPOCH`).
    fn unwind(&self) {
        if let Some((base, epoch)) = self.owned
            && epoch == saved_fds_epoch()
        {
            unwind_saved_fds(base);
        }
    }

    /// Number of saved fds this state currently holds.
    #[cfg(test)]
    fn saved_len(&self) -> usize {
        match self.owned {
            Some((base, epoch)) if epoch == saved_fds_epoch() => {
                saved_fds_len().saturating_sub(base)
            }
            _ => 0,
        }
    }

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
            ExpandedOp::Dup(src) => {
                if src == "-" {
                    if save {
                        self.save_fd(target_fd)?;
                    }
                    unsafe { libc::close(target_fd) };
                    Ok(())
                } else {
                    let src_fd: RawFd = src
                        .parse()
                        .map_err(|_| format!("{}: invalid file descriptor", src))?;
                    if src_fd != target_fd {
                        if save {
                            self.save_fd(target_fd)?;
                        }
                        raw_dup2(src_fd, target_fd)
                            .map_err(|e| format!("{}: {}", src, errno_text(e)))?;
                    }
                    Ok(())
                }
            }
            ExpandedOp::HereDoc(body) => self.install(target_fd, save, || heredoc_source_fd(body)),
        }
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
        if save {
            self.save_fd(target_fd)?;
        }
        let fd = open()?;
        if fd != target_fd {
            let res = raw_dup2(fd, target_fd);
            unsafe { libc::close(fd) };
            res.map_err(|e| format!("dup2: {}", errno_text(e)))?;
        }
        Ok(())
    }

    /// dup() the fd before it gets overwritten, storing the saved copy.
    /// Uses F_DUPFD_CLOEXEC so the saved fd is automatically closed in child processes.
    /// A closed fd (EBADF) is recorded as [`WAS_CLOSED`] so `restore`
    /// closes it again — redirecting a closed fd is not an error
    /// (`{ echo x >&5; } 5>&1` works in every shell).
    fn save_fd(&mut self, fd: RawFd) -> Result<(), String> {
        self.claim();
        let saved = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        if saved == -1 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EBADF) {
                SAVED_FDS.with(|s| s.borrow_mut().push((fd, WAS_CLOSED, false)));
                return Ok(());
            }
            return Err(format!("dup: {}", err));
        }
        // dup2 on restore clears FD_CLOEXEC; remember it so an internal
        // close-on-exec fd (the signal self-pipe, the terminal fd) that a
        // user redirection temporarily overwrote does not leak into
        // children afterwards (`: 10>&1; external` with a posix_spawn
        // child would otherwise inherit the self-pipe).
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        let cloexec = flags != -1 && flags & libc::FD_CLOEXEC != 0;
        SAVED_FDS.with(|s| s.borrow_mut().push((fd, saved, cloexec)));
        Ok(())
    }

    /// Restore all saved fds in reverse order (LIFO).
    pub fn restore(&mut self) {
        self.unwind();
    }
}

impl Drop for RedirectState {
    fn drop(&mut self) {
        // A state dropped without `restore` (an early return) must not
        // leave its redirections in place for the commands after it, nor
        // leave stale entries on the shared stack.
        self.unwind();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{Redirect, RedirectKind, Word};

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    // Tests that manipulate process-wide file descriptors (fd 0, fd 1,
    // fd 2) hold `test_sync::lock_stdio_fds` for their entire duration.

    #[test]
    fn test_redirect_output_and_restore() {
        let _guard = crate::test_sync::lock_stdio_fds();
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
        let _guard = crate::test_sync::lock_stdio_fds();
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
        let _guard = crate::test_sync::lock_stdio_fds();
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
            state.saved_len() == 0,
            "saved_fds should be empty after rollback, got {} entries",
            state.saved_len()
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

    /// Redirecting an fd that is closed records it as closed and closes it
    /// again on restore — even when `open` hands back the target number
    /// itself (the file must not leak past the command).
    #[test]
    fn closed_target_fd_is_closed_again_on_restore() {
        let _guard = crate::test_sync::lock_stdio_fds();
        let mut env = make_env();
        // The highest fd the process may open: parallel tests allocate
        // fds lowest-first, so none of them owns this number (a fixed
        // mid-range number such as 77 could be a parallel test's pipe,
        // which closing here would break).
        let mut lim: libc::rlimit = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) }, 0);
        let target: RawFd = (lim.rlim_cur.min(1024) - 1) as RawFd;
        assert_eq!(
            unsafe { libc::fcntl(target, libc::F_GETFD) },
            -1,
            "fd {target} unexpectedly open"
        );
        let tmp = std::env::temp_dir().join("yosh_redirect_closed_target.txt");
        let redirects = vec![Redirect {
            fd: Some(target),
            kind: RedirectKind::Output(Word::literal(tmp.to_str().unwrap())),
        }];
        let mut state = RedirectState::new();
        state
            .apply(&redirects, &mut env, true)
            .expect("redirecting a closed fd must succeed");
        assert_ne!(unsafe { libc::fcntl(target, libc::F_GETFD) }, -1);
        state.restore();
        assert_eq!(
            unsafe { libc::fcntl(target, libc::F_GETFD) },
            -1,
            "fd must be closed again after restore"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    /// A state with no redirections claims nothing on the saved-fd
    /// stack (it is created for every command), and dropping it inside
    /// an enclosing state that did save must not undo the outer
    /// redirection.
    #[test]
    fn empty_state_does_not_touch_the_saved_fd_stack() {
        let _guard = crate::test_sync::lock_stdio_fds();
        let mut env = make_env();
        // A high fd no parallel test owns (see the test above).
        let mut lim: libc::rlimit = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) }, 0);
        let target: RawFd = (lim.rlim_cur.min(1024) - 2) as RawFd;
        let tmp = std::env::temp_dir().join("yosh_redirect_empty_state.txt");
        let redirects = vec![Redirect {
            fd: Some(target),
            kind: RedirectKind::Output(Word::literal(tmp.to_str().unwrap())),
        }];
        let was_open = unsafe { libc::fcntl(target, libc::F_GETFD) } != -1;
        let mut outer = RedirectState::new();
        outer.apply(&redirects, &mut env, true).expect("apply");
        let depth = saved_fds_len();
        {
            let mut inner = RedirectState::new();
            inner.apply_expanded(&[], true).expect("empty apply");
            assert!(inner.owned.is_none());
            assert_eq!(inner.saved_len(), 0);
            inner.restore();
        }
        assert_eq!(saved_fds_len(), depth, "inner drop popped outer entries");
        assert_eq!(outer.saved_len(), 1);
        outer.restore();
        assert_eq!(
            unsafe { libc::fcntl(target, libc::F_GETFD) } != -1,
            was_open,
            "outer restore must put fd {target} back"
        );
        let _ = std::fs::remove_file(&tmp);
    }
}
