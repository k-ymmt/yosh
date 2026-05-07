//! POSIX-style job formatting for the `jobs` builtin and related
//! status reporting.
//!
//! Two output forms:
//! - `format_job`: short form `[n]+  Status  command`
//! - `format_job_long`: long form `[n]+ PID  Status  command`
//!
//! The indicator character is `+` for the current job, `-` for the
//! previous job, and a space otherwise.

use super::JobId;

impl super::JobTable {
    /// Format a job in POSIX short form: `[n]+  Status  command`
    ///
    /// The indicator character is `+` for the current job, `-` for the
    /// previous job, and a space otherwise.
    pub fn format_job(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        Some(format!(
            "[{}]{}  {}  {}",
            job.id, indicator, job.status, job.command
        ))
    }

    /// Format a job in long form: `[n]+ PID  Status  command`
    pub fn format_job_long(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        Some(format!(
            "[{}]{} {}  {}  {}",
            job.id,
            indicator,
            job.pgid.as_raw(),
            job.status,
            job.command
        ))
    }

    fn indicator(&self, id: JobId) -> char {
        if self.current == Some(id) {
            '+'
        } else if self.previous == Some(id) {
            '-'
        } else {
            ' '
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::env::jobs::{JobStatus, JobTable};
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    #[test]
    fn test_format_job_running() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(100), vec![pid(100)], "sleep 10", false);
        let s = table.format_job(id).expect("format should succeed");
        assert!(s.contains("[1]"), "should contain job id");
        assert!(s.contains('+'), "current job should have + indicator");
        assert!(s.contains("Running"), "should contain Running status");
        assert!(s.contains("sleep 10"), "should contain command");
    }

    #[test]
    fn test_format_job_done() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(200), vec![pid(200)], "true", false);
        table.update_status(pid(200), JobStatus::Done(0));
        let s = table.format_job(id).expect("format should succeed");
        assert!(s.contains("Done"), "should contain Done status");
    }

    #[test]
    fn test_format_job_nonexistent() {
        let table = JobTable::default();
        assert!(table.format_job(99).is_none());
    }
}
