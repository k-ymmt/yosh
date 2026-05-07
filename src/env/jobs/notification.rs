//! Notification state machine for terminated jobs.
//!
//! The interactive shell reports each finished job to the user
//! exactly once. This module centralizes the predicates that
//! distinguish "newly finished" from "already announced" from
//! "still alive".
//!
//! State transitions:
//! - `Running`/`Stopped` → not notifiable, not cleanable.
//! - `Done`/`Terminated` with `notified == false` → notifiable.
//! - `Done`/`Terminated` with `notified == true` → cleanable.
//!
//! `JobTable::update_status` resets `notified` whenever a job's
//! status changes so a job that was reported, then re-runs (e.g.,
//! `bg` after `Stopped`), gets re-reported on its next finish.

use super::{Job, JobId};

/// True if the job has finished but the user has not yet been told.
pub(super) fn is_notifiable(job: &Job) -> bool {
    !job.notified && job.status.is_terminal()
}

/// True if the job has finished AND the user has been told — safe to remove.
pub(super) fn is_cleanable(job: &Job) -> bool {
    job.notified && job.status.is_terminal()
}

/// Reset notification state after a status change. Called from
/// `JobTable::update_status` whenever the status mutates so the
/// new state will be reported.
pub(super) fn reset_after_status_change(job: &mut Job) {
    job.notified = false;
}

impl super::JobTable {
    /// Return ids of jobs that have finished (Done or Terminated) but have
    /// not yet been notified, sorted in ascending order.
    ///
    /// Stopped jobs are excluded — they are notified immediately at stop time
    /// by the caller, not deferred.
    pub fn pending_notifications(&self) -> Vec<JobId> {
        let mut ids: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| is_notifiable(j))
            .map(|j| j.id)
            .collect();
        ids.sort();
        ids
    }

    /// Mark a job as notified (the status change has been reported to the
    /// user).
    pub fn mark_notified(&mut self, id: JobId) {
        if let Some(job) = self.jobs.get_mut(&id) {
            job.notified = true;
        }
    }

    /// Remove all jobs that are both notified AND in a terminal state
    /// (Done or Terminated).
    pub fn cleanup_notified(&mut self) {
        let to_remove: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| is_cleanable(j))
            .map(|j| j.id)
            .collect();
        for id in to_remove {
            self.remove_job(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::{JobStatus, JobTable};
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    fn job_with(status: JobStatus, notified: bool) -> Job {
        Job {
            id: 1,
            pgid: pid(1),
            pids: vec![pid(1)],
            command: "x".to_string(),
            status,
            notified,
            foreground: false,
            saved_tmodes: None,
        }
    }

    // -----------------------------------------------------------------------
    // Predicate tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_notifiable_done_unnotified() {
        assert!(is_notifiable(&job_with(JobStatus::Done(0), false)));
    }

    #[test]
    fn test_is_notifiable_done_already_notified() {
        assert!(!is_notifiable(&job_with(JobStatus::Done(0), true)));
    }

    #[test]
    fn test_is_notifiable_running_is_false() {
        assert!(!is_notifiable(&job_with(JobStatus::Running, false)));
    }

    #[test]
    fn test_is_cleanable_terminated_notified() {
        assert!(is_cleanable(&job_with(JobStatus::Terminated(9), true)));
    }

    #[test]
    fn test_is_cleanable_unnotified_is_false() {
        assert!(!is_cleanable(&job_with(JobStatus::Done(0), false)));
    }

    #[test]
    fn test_reset_after_status_change_clears_notified() {
        let mut job = job_with(JobStatus::Running, true);
        reset_after_status_change(&mut job);
        assert!(!job.notified);
    }

    // -----------------------------------------------------------------------
    // pending_notifications
    // -----------------------------------------------------------------------

    #[test]
    fn test_pending_notifications_empty_when_running() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep", false);
        assert!(table.pending_notifications().is_empty());
    }

    #[test]
    fn test_pending_notifications_non_empty_when_done() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "ls", false);
        table.update_status(pid(1), JobStatus::Done(0));

        let pending = table.pending_notifications();
        assert_eq!(pending, vec![id]);
    }

    #[test]
    fn test_pending_notifications_sorted() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        table.update_status(pid(2), JobStatus::Done(0));
        table.update_status(pid(1), JobStatus::Terminated(9));

        let pending = table.pending_notifications();
        assert_eq!(pending, vec![id1, id2]);
    }

    // -----------------------------------------------------------------------
    // mark_notified clears pending
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_notified_clears_pending() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "ls", false);
        table.update_status(pid(1), JobStatus::Done(0));
        assert!(!table.pending_notifications().is_empty());

        table.mark_notified(id);
        assert!(table.pending_notifications().is_empty());
    }
}
