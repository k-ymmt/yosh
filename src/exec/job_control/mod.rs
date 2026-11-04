//! Job-control builtins (`wait`, `jobs`, `fg`, `bg`) and foreground-job
//! waiting.
//!
//! * `wait` — the `wait` builtin and its per-pid wait loop.
//! * `jobs` — the `jobs` builtin and job-status notifications.
//! * `fg_bg` — the `fg` / `bg` builtins.
//! * `foreground` — waiting for a foreground job to exit or stop.

mod fg_bg;
mod foreground;
mod jobs;
mod wait;

use crate::env::jobs::JobStatus;

/// Strip the leading `%` (and optional `?`) from a job spec string for
/// inclusion in error messages. Matches bash: `wait %sleep` with ambiguous
/// match reports `wait: sleep: ambiguous job spec`, not `%sleep:`.
/// Inputs that don't start with `%` are returned unchanged.
fn strip_job_spec_prefix(spec: &str) -> &str {
    match spec.strip_prefix('%') {
        Some(rest) => rest.strip_prefix('?').unwrap_or(rest),
        None => spec,
    }
}

/// The wait-style status (`code` or `128+sig`) of a job member that has
/// terminated; `None` while it is still running or stopped.
fn terminated_wait_status(status: JobStatus) -> Option<i32> {
    match status {
        JobStatus::Done(code) => Some(code),
        JobStatus::Terminated(sig) => Some(128 + sig),
        _ => None,
    }
}

/// Compute a job's wait status from its members' wait statuses
/// (pipeline order). POSIX: a pipeline's status is its last element's;
/// with pipefail, the last nonzero member status (bash applies pipefail
/// to background-job waits too — `set -o pipefail; false | true &
/// wait %1` is 1, empirical 2026-09-02). Any `None` entry makes the
/// whole job report 127 — see the body comment.
fn job_wait_status(statuses: &[Option<i32>], pipefail: bool) -> i32 {
    // A None entry is a member whose status could not be determined
    // (waitpid error with nothing remembered — e.g. waiting a parent's
    // job from a subshell): the job as a whole is then not waitable and
    // reports 127, like bash (adversarial review round 3: [Some(0),
    // None] must not collapse to the successful member's 0).
    let mut resolved = Vec::with_capacity(statuses.len());
    for s in statuses {
        match s {
            Some(code) => resolved.push(*code),
            None => return 127,
        }
    }
    if pipefail && let Some(&code) = resolved.iter().rev().find(|&&c| c != 0) {
        return code;
    }
    resolved.last().copied().unwrap_or(127)
}
