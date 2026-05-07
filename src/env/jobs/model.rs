//! Core data types for the job table: `Job`, `JobStatus`, and `JobId`.
//!
//! These types are observed by `exec/job_control`, `exec/control`, and
//! the `jobs` builtin. Public-API names and signatures are preserved.

pub type JobId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped(i32),    // signal number (e.g. SIGTSTP=20)
    Done(i32),       // exit code
    Terminated(i32), // killed by signal number
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Running => write!(f, "Running"),
            JobStatus::Stopped(sig) => {
                let name = crate::signal::signal_number_to_name(*sig).unwrap_or("UNKNOWN");
                write!(f, "Stopped(SIG{})", name)
            }
            JobStatus::Done(0) => write!(f, "Done"),
            JobStatus::Done(code) => write!(f, "Done({})", code),
            JobStatus::Terminated(sig) => {
                let name = crate::signal::signal_number_to_name(*sig).unwrap_or("UNKNOWN");
                write!(f, "Terminated(SIG{})", name)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub pgid: nix::unistd::Pid,
    pub pids: Vec<nix::unistd::Pid>,
    pub command: String,
    pub status: JobStatus,
    pub notified: bool,
    pub foreground: bool,
    /// Termios snapshot captured when the job last stopped (SIGTSTP/SIGSTOP).
    /// Used as the restore target on `fg`. `None` for jobs that have never
    /// been stopped, or on non-interactive / non-monitor shell modes.
    pub(super) saved_tmodes: Option<nix::sys::termios::Termios>,
}

impl Job {
    /// Termios snapshot captured the last time this job stopped
    /// (SIGTSTP/SIGSTOP), or `None` if it has never stopped or capture was
    /// unavailable (non-interactive/non-monitor or stdin not a TTY).
    pub fn saved_tmodes(&self) -> Option<&nix::sys::termios::Termios> {
        self.saved_tmodes.as_ref()
    }

    /// Replace the saved termios snapshot. Intended only for the
    /// `WaitStatus::Stopped` branch of foreground-wait — passing `None`
    /// is valid and clears any previously stored value, which is what
    /// the GNU libc manual job-control pattern requires after a
    /// mid-session `exec 0</dev/null` redirects stdin away from the TTY.
    pub fn set_saved_tmodes(&mut self, t: Option<nix::sys::termios::Termios>) {
        self.saved_tmodes = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::JobTable;
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    #[test]
    fn test_job_status_equality() {
        assert_eq!(JobStatus::Running, JobStatus::Running);
        assert_eq!(JobStatus::Done(0), JobStatus::Done(0));
        assert_ne!(JobStatus::Done(0), JobStatus::Done(1));
        assert_eq!(JobStatus::Stopped(20), JobStatus::Stopped(20));
        assert_eq!(JobStatus::Terminated(9), JobStatus::Terminated(9));
    }

    #[test]
    fn test_job_saved_tmodes_defaults_none() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(42), vec![pid(42)], "cmd", false);
        let job = table.get(id).expect("job should exist");
        assert!(
            job.saved_tmodes().is_none(),
            "saved_tmodes() should default to None on new job"
        );
    }

    #[test]
    fn test_job_set_saved_tmodes_overwrites_with_none() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(42), vec![pid(42)], "cmd", false);

        let zeroed: libc::termios = unsafe { std::mem::zeroed() };
        let t: nix::sys::termios::Termios = zeroed.into();

        table
            .get_mut(id)
            .expect("job should exist")
            .set_saved_tmodes(Some(t));
        assert!(
            table.get(id).unwrap().saved_tmodes().is_some(),
            "saved_tmodes() should return Some after set_saved_tmodes(Some(_))"
        );

        table
            .get_mut(id)
            .expect("job should exist")
            .set_saved_tmodes(None);
        assert!(
            table.get(id).unwrap().saved_tmodes().is_none(),
            "saved_tmodes() should return None after set_saved_tmodes(None)"
        );
    }

    // -----------------------------------------------------------------------
    // Display for JobStatus
    // -----------------------------------------------------------------------

    #[test]
    fn test_display_running() {
        assert_eq!(JobStatus::Running.to_string(), "Running");
    }

    #[test]
    fn test_display_done_success() {
        // Done(0) renders as bare "Done" (no exit code)
        assert_eq!(JobStatus::Done(0).to_string(), "Done");
    }

    #[test]
    fn test_display_done_nonzero() {
        assert_eq!(JobStatus::Done(2).to_string(), "Done(2)");
        assert_eq!(JobStatus::Done(127).to_string(), "Done(127)");
    }

    #[test]
    fn test_display_stopped_known_signal() {
        // SIGTSTP = 20 on Linux/macOS — signal_number_to_name returns "TSTP"
        let s = JobStatus::Stopped(20).to_string();
        assert!(s.starts_with("Stopped(SIG"), "got: {}", s);
    }

    #[test]
    fn test_display_stopped_unknown_signal() {
        // 99 is not a real signal — falls back to "UNKNOWN"
        assert_eq!(JobStatus::Stopped(99).to_string(), "Stopped(SIGUNKNOWN)");
    }

    #[test]
    fn test_display_terminated_known_signal() {
        // SIGKILL = 9
        let s = JobStatus::Terminated(9).to_string();
        assert!(s.starts_with("Terminated(SIG"), "got: {}", s);
    }

    #[test]
    fn test_display_terminated_unknown_signal() {
        assert_eq!(JobStatus::Terminated(99).to_string(), "Terminated(SIGUNKNOWN)");
    }
}
