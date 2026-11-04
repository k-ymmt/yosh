//! Expanding redirection words (and here-document bodies) in the shell
//! process, ahead of applying them.

use std::os::fd::RawFd;

use crate::env::ShellEnv;
use crate::error::{RuntimeErrorKind, ShellError};
use crate::expand::expand_word_to_string;
use crate::parser::ast::{Redirect, RedirectKind, Word};

/// What a redirection does once its word has been expanded.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExpandedOp {
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
    pub(super) fd: RawFd,
    pub(super) op: ExpandedOp,
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
