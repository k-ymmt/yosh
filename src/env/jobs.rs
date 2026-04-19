use nix::unistd::Pid;
use std::collections::HashMap;
use std::os::fd::BorrowedFd;
use std::os::unix::io::RawFd;

pub type JobId = u32;

// ---------------------------------------------------------------------------
// JobStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped(i32),    // signal number (e.g. SIGTSTP=20)
    Done(i32),       // exit code
    Terminated(i32), // killed by signal number
}

// ---------------------------------------------------------------------------
// Job
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub pgid: Pid,
    pub pids: Vec<Pid>,
    pub command: String,
    pub status: JobStatus,
    pub notified: bool,
    pub foreground: bool,
}

// ---------------------------------------------------------------------------
// JobSpec (POSIX §3.204 Job Control Job ID)
// ---------------------------------------------------------------------------

/// Parsed form of a POSIX job specifier string such as `%%`, `%1`, `%vim`.
///
/// Borrows from the input string so parsing is zero-allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpec<'a> {
    /// `%%` or `%+` — current job
    Current,
    /// `%-` — previous job
    Previous,
    /// `%n` — job with numeric id
    Numeric(JobId),
    /// `%string` — command begins with string
    Prefix(&'a str),
    /// `%?string` — command contains string
    Substring(&'a str),
}

/// Error returned by `parse_job_spec` and `JobTable::resolve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpecError {
    /// Input is not a syntactically valid job specifier.
    Malformed,
    /// Parse succeeded but no job matches the spec.
    NoSuchJob,
    /// A Prefix or Substring spec matched two or more jobs.
    Ambiguous,
}

/// Parse a POSIX job specifier string into a `JobSpec`.
///
/// Disambiguation order (earliest match wins):
/// 1. `"%%"` / `"%+"` → `Current`
/// 2. `"%-"` → `Previous`
/// 3. `"%<digits>"` with non-empty digit run → `Numeric`
/// 4. `"%?<rest>"` with non-empty `rest` → `Substring`
/// 5. `"%<rest>"` with non-empty `rest` → `Prefix`
/// 6. Otherwise → `Malformed`
pub fn parse_job_spec(s: &str) -> Result<JobSpec<'_>, JobSpecError> {
    let rest = s.strip_prefix('%').ok_or(JobSpecError::Malformed)?;

    match rest {
        "" => Err(JobSpecError::Malformed),
        "%" | "+" => Ok(JobSpec::Current),
        "-" => Ok(JobSpec::Previous),
        _ => {
            // Pure digit run → Numeric
            if rest.bytes().all(|b| b.is_ascii_digit()) {
                return rest
                    .parse::<JobId>()
                    .map(JobSpec::Numeric)
                    .map_err(|_| JobSpecError::Malformed);
            }

            // "?<rest>" → Substring
            if let Some(sub) = rest.strip_prefix('?') {
                if sub.is_empty() {
                    return Err(JobSpecError::Malformed);
                }
                return Ok(JobSpec::Substring(sub));
            }

            // Everything else with non-empty rest → Prefix
            Ok(JobSpec::Prefix(rest))
        }
    }
}

// ---------------------------------------------------------------------------
// JobTable
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct JobTable {
    jobs: HashMap<JobId, Job>,
    next_id: JobId,
    current: Option<JobId>,
    previous: Option<JobId>,
}

impl JobTable {
    // -----------------------------------------------------------------------
    // Task 2: add_job, remove_job, accessors
    // -----------------------------------------------------------------------

    /// Add a new job. Returns the assigned JobId.
    /// The new job becomes current; the old current becomes previous.
    pub fn add_job(
        &mut self,
        pgid: Pid,
        pids: Vec<Pid>,
        command: impl Into<String>,
        foreground: bool,
    ) -> JobId {
        self.next_id += 1;
        let id = self.next_id;

        let job = Job {
            id,
            pgid,
            pids,
            command: command.into(),
            status: JobStatus::Running,
            notified: false,
            foreground,
        };

        self.jobs.insert(id, job);

        // The new job becomes current; old current becomes previous.
        self.previous = self.current;
        self.current = Some(id);

        id
    }

    /// Remove a job from the table.
    /// If the removed job was current, previous becomes current and a new
    /// previous is found (the next most-recent remaining job).
    pub fn remove_job(&mut self, id: JobId) {
        self.jobs.remove(&id);

        if self.current == Some(id) {
            // Promote previous to current.
            self.current = self.previous;

            // Find a new previous: highest id that is not the new current.
            let new_current = self.current;
            self.previous = self
                .jobs
                .keys()
                .copied()
                .filter(|&k| Some(k) != new_current)
                .max();
        } else if self.previous == Some(id) {
            // Previous was removed — find the next most-recent job that is
            // not the current one.
            let cur = self.current;
            self.previous = self.jobs.keys().copied().filter(|&k| Some(k) != cur).max();
        }
    }

    /// Get a shared reference to a job by id.
    pub fn get(&self, id: JobId) -> Option<&Job> {
        self.jobs.get(&id)
    }

    /// Get a mutable reference to a job by id.
    pub fn get_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.get_mut(&id)
    }

    /// Return a reference to the current (most recent) job.
    #[allow(dead_code)] // tested; will be used by `fg`/`bg` builtins
    pub fn current_job(&self) -> Option<&Job> {
        self.current.and_then(|id| self.jobs.get(&id))
    }

    /// Return a reference to the previous job.
    #[allow(dead_code)] // tested; will be used by `fg`/`bg` builtins
    pub fn previous_job(&self) -> Option<&Job> {
        self.previous.and_then(|id| self.jobs.get(&id))
    }

    /// Return the id of the current job.
    pub fn current_id(&self) -> Option<JobId> {
        self.current
    }

    /// Return the id of the previous job.
    #[allow(dead_code)] // tested; will be used by `fg`/`bg` builtins
    pub fn previous_id(&self) -> Option<JobId> {
        self.previous
    }

    /// Return true if no jobs are tracked.
    #[allow(dead_code)] // tested; standard container API
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    // -----------------------------------------------------------------------
    // Task 3: status updates, search helpers
    // -----------------------------------------------------------------------

    /// Update the status of the job that contains `pid`.
    /// Resets the notified flag to false so the change will be reported.
    pub fn update_status(&mut self, pid: Pid, status: JobStatus) {
        if let Some(job) = self.jobs.values_mut().find(|j| j.pids.contains(&pid)) {
            job.status = status;
            job.notified = false;
        }
    }

    /// Find a job by its process group id (shared reference).
    #[allow(dead_code)] // tested; will be used by job specifier lookups
    pub fn find_by_pgid(&self, pgid: Pid) -> Option<&Job> {
        self.jobs.values().find(|j| j.pgid == pgid)
    }

    /// Find a job by its process group id (mutable reference).
    #[allow(dead_code)] // tested; will be used by job specifier lookups
    pub fn find_by_pgid_mut(&mut self, pgid: Pid) -> Option<&mut Job> {
        self.jobs.values_mut().find(|j| j.pgid == pgid)
    }

    /// Return the pgid of the most recent background job (highest id where
    /// `!foreground`).  Returns `None` if no background jobs exist.
    pub fn last_bg_pid(&self) -> Option<Pid> {
        self.jobs
            .values()
            .filter(|j| !j.foreground)
            .max_by_key(|j| j.id)
            .map(|j| j.pgid)
    }

    /// Iterate over all jobs sorted by id (ascending).
    pub fn all_jobs(&self) -> impl Iterator<Item = &Job> {
        let mut ids: Vec<JobId> = self.jobs.keys().copied().collect();
        ids.sort();
        // Collect into Vec so we own the sorted order.
        let sorted: Vec<&Job> = ids.iter().map(|id| &self.jobs[id]).collect();
        sorted.into_iter()
    }

    // -----------------------------------------------------------------------
    // Task 4: spec resolution, notifications, formatting, cleanup
    // -----------------------------------------------------------------------

    /// Resolve a job specification string to a JobId.
    ///
    /// Supported forms (see `parse_job_spec` for syntax):
    /// - `%%` / `%+` — current job
    /// - `%-` — previous job
    /// - `%n` — job by numeric id
    /// - `%string` — command begins with string
    /// - `%?string` — command contains string
    ///
    /// Returns `Err(Ambiguous)` when a Prefix/Substring spec matches 2+ jobs.
    pub fn resolve_job_spec(&self, spec: &str) -> Result<JobId, JobSpecError> {
        self.resolve(parse_job_spec(spec)?)
    }

    /// Resolve a parsed `JobSpec` to a `JobId`.
    ///
    /// Matching is performed against `Job.command` (full command line),
    /// case-sensitive, across all job statuses (Running, Stopped, Done,
    /// Terminated) — bash-compatible.
    ///
    /// Returns:
    /// - `Ok(id)` if exactly one job matches
    /// - `Err(NoSuchJob)` if no job matches
    /// - `Err(Ambiguous)` if two or more jobs match (Prefix/Substring only)
    pub fn resolve(&self, spec: JobSpec<'_>) -> Result<JobId, JobSpecError> {
        match spec {
            JobSpec::Current => self.current.ok_or(JobSpecError::NoSuchJob),
            JobSpec::Previous => self.previous.ok_or(JobSpecError::NoSuchJob),
            JobSpec::Numeric(n) => {
                if self.jobs.contains_key(&n) {
                    Ok(n)
                } else {
                    Err(JobSpecError::NoSuchJob)
                }
            }
            JobSpec::Prefix(s) => self.resolve_by(|cmd| cmd.starts_with(s)),
            JobSpec::Substring(s) => self.resolve_by(|cmd| cmd.contains(s)),
        }
    }

    /// Internal helper: scan all jobs and collapse match count to a Result.
    fn resolve_by<F>(&self, mut pred: F) -> Result<JobId, JobSpecError>
    where
        F: FnMut(&str) -> bool,
    {
        let mut matched: Option<JobId> = None;
        for job in self.jobs.values() {
            if pred(&job.command) {
                if matched.is_some() {
                    return Err(JobSpecError::Ambiguous);
                }
                matched = Some(job.id);
            }
        }
        matched.ok_or(JobSpecError::NoSuchJob)
    }

    /// Return ids of jobs that have finished (Done or Terminated) but have
    /// not yet been notified, sorted in ascending order.
    ///
    /// Stopped jobs are excluded — they are notified immediately at stop time
    /// by the caller, not deferred.
    pub fn pending_notifications(&self) -> Vec<JobId> {
        let mut ids: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| {
                !j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_))
            })
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

    /// Format a job in POSIX short form: `[n]+  Status  command`
    ///
    /// The indicator character is `+` for the current job, `-` for the
    /// previous job, and a space otherwise.
    pub fn format_job(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        let status_str = self.format_status(job.status);
        Some(format!(
            "[{}]{}  {}  {}",
            job.id, indicator, status_str, job.command
        ))
    }

    /// Format a job in long form: `[n]+ PID  Status  command`
    pub fn format_job_long(&self, id: JobId) -> Option<String> {
        let job = self.jobs.get(&id)?;
        let indicator = self.indicator(id);
        let status_str = self.format_status(job.status);
        Some(format!(
            "[{}]{} {}  {}  {}",
            job.id,
            indicator,
            job.pgid.as_raw(),
            status_str,
            job.command
        ))
    }

    /// Remove all jobs that are both notified AND in a terminal state
    /// (Done or Terminated).
    pub fn cleanup_notified(&mut self) {
        let to_remove: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| {
                j.notified && matches!(j.status, JobStatus::Done(_) | JobStatus::Terminated(_))
            })
            .map(|j| j.id)
            .collect();
        for id in to_remove {
            self.remove_job(id);
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn indicator(&self, id: JobId) -> char {
        if self.current == Some(id) {
            '+'
        } else if self.previous == Some(id) {
            '-'
        } else {
            ' '
        }
    }

    fn format_status(&self, status: JobStatus) -> String {
        match status {
            JobStatus::Running => "Running".to_string(),
            JobStatus::Stopped(sig) => {
                let name = crate::signal::signal_number_to_name(sig).unwrap_or("UNKNOWN");
                format!("Stopped(SIG{})", name)
            }
            JobStatus::Done(0) => "Done".to_string(),
            JobStatus::Done(code) => format!("Done({})", code),
            JobStatus::Terminated(sig) => {
                let name = crate::signal::signal_number_to_name(sig).unwrap_or("UNKNOWN");
                format!("Terminated(SIG{})", name)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Task 5: Terminal control
// ---------------------------------------------------------------------------

const TERMINAL_FD: RawFd = 0;

/// Give the terminal to the specified process group.
pub fn give_terminal(pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: TERMINAL_FD (0) is stdin, which lives for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(TERMINAL_FD) };
    nix::unistd::tcsetpgrp(fd, pgid)
}

/// Reclaim the terminal for the shell process group.
pub fn take_terminal(shell_pgid: Pid) -> Result<(), nix::Error> {
    // SAFETY: TERMINAL_FD (0) is stdin, which lives for the process lifetime.
    let fd = unsafe { BorrowedFd::borrow_raw(TERMINAL_FD) };
    nix::unistd::tcsetpgrp(fd, shell_pgid)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    // -----------------------------------------------------------------------
    // Default / empty
    // -----------------------------------------------------------------------

    #[test]
    fn test_default_is_empty() {
        let table = JobTable::default();
        assert!(table.is_empty());
        assert!(table.current_job().is_none());
        assert!(table.previous_job().is_none());
    }

    // -----------------------------------------------------------------------
    // JobStatus equality
    // -----------------------------------------------------------------------

    #[test]
    fn test_job_status_equality() {
        assert_eq!(JobStatus::Running, JobStatus::Running);
        assert_eq!(JobStatus::Done(0), JobStatus::Done(0));
        assert_ne!(JobStatus::Done(0), JobStatus::Done(1));
        assert_eq!(JobStatus::Stopped(20), JobStatus::Stopped(20));
        assert_eq!(JobStatus::Terminated(9), JobStatus::Terminated(9));
    }

    // -----------------------------------------------------------------------
    // add_job: incrementing IDs starting from 1
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_job_assigns_incrementing_ids() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(100), vec![pid(100)], "sleep 1", false);
        let id2 = table.add_job(pid(200), vec![pid(200)], "sleep 2", false);
        let id3 = table.add_job(pid(300), vec![pid(300)], "sleep 3", false);

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    // -----------------------------------------------------------------------
    // add_job: current / previous updates
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_job_updates_current_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(100), vec![pid(100)], "cmd1", false);
        assert_eq!(table.current_id(), Some(id1));
        assert!(table.previous_id().is_none());

        let id2 = table.add_job(pid(200), vec![pid(200)], "cmd2", false);
        assert_eq!(table.current_id(), Some(id2));
        assert_eq!(table.previous_id(), Some(id1));

        let id3 = table.add_job(pid(300), vec![pid(300)], "cmd3", false);
        assert_eq!(table.current_id(), Some(id3));
        assert_eq!(table.previous_id(), Some(id2));
    }

    // -----------------------------------------------------------------------
    // get / get_mut
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_returns_correct_job() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(42), vec![pid(42)], "echo hi", false);
        let job = table.get(id).expect("job should exist");
        assert_eq!(job.command, "echo hi");
        assert_eq!(job.pgid, pid(42));
    }

    #[test]
    fn test_get_returns_none_for_nonexistent() {
        let table = JobTable::default();
        assert!(table.get(99).is_none());
    }

    #[test]
    fn test_get_mut_modifies_job() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(10), vec![pid(10)], "cmd", false);
        table.get_mut(id).unwrap().status = JobStatus::Done(0);
        assert_eq!(table.get(id).unwrap().status, JobStatus::Done(0));
    }

    // -----------------------------------------------------------------------
    // remove_job: current / previous updates
    // -----------------------------------------------------------------------

    #[test]
    fn test_remove_job_updates_current_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        let id3 = table.add_job(pid(3), vec![pid(3)], "c", false);
        // current=3, previous=2

        table.remove_job(id3);
        // After removing current (3), previous (2) becomes current.
        assert_eq!(table.current_id(), Some(id2));
        // New previous should be the remaining job (1).
        assert_eq!(table.previous_id(), Some(id1));
    }

    #[test]
    fn test_remove_non_current_job() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        // current=2, previous=1

        table.remove_job(id1);
        // current stays 2; previous was 1, now gone → None
        assert_eq!(table.current_id(), Some(id2));
        assert!(table.previous_id().is_none());
    }

    // -----------------------------------------------------------------------
    // current_job / previous_job
    // -----------------------------------------------------------------------

    #[test]
    fn test_current_job_previous_job() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(10), vec![pid(10)], "first", false);
        let id2 = table.add_job(pid(20), vec![pid(20)], "second", false);

        assert_eq!(table.current_job().map(|j| j.id), Some(id2));
        assert_eq!(table.previous_job().map(|j| j.id), Some(id1));
    }

    // -----------------------------------------------------------------------
    // update_status
    // -----------------------------------------------------------------------

    #[test]
    fn test_update_status_by_pid() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(55), vec![pid(55), pid(56)], "pipe", false);
        table.update_status(pid(56), JobStatus::Done(0));

        let job = table.get(id).unwrap();
        assert_eq!(job.status, JobStatus::Done(0));
        assert!(!job.notified, "notified should be reset to false");
    }

    #[test]
    fn test_update_status_unknown_pid_is_noop() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(70), vec![pid(70)], "sleep", false);
        // Update a PID not in the table — should be silent no-op.
        table.update_status(pid(9999), JobStatus::Done(1));
        // Original job untouched.
        assert_eq!(table.get(id).unwrap().status, JobStatus::Running);
    }

    // -----------------------------------------------------------------------
    // find_by_pgid
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_by_pgid() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(77), vec![pid(77)], "foo", false);
        let found = table.find_by_pgid(pid(77)).expect("should find by pgid");
        assert_eq!(found.id, id);
        assert!(table.find_by_pgid(pid(9999)).is_none());
    }

    // -----------------------------------------------------------------------
    // last_bg_pid
    // -----------------------------------------------------------------------

    #[test]
    fn test_last_bg_pid_none_when_empty() {
        let table = JobTable::default();
        assert!(table.last_bg_pid().is_none());
    }

    #[test]
    fn test_last_bg_pid_returns_most_recent_bg_job() {
        let mut table = JobTable::default();
        table.add_job(pid(10), vec![pid(10)], "bg1", false); // background
        table.add_job(pid(20), vec![pid(20)], "fg", true); // foreground — should be excluded
        table.add_job(pid(30), vec![pid(30)], "bg2", false); // background (most recent)

        assert_eq!(table.last_bg_pid(), Some(pid(30)));
    }

    #[test]
    fn test_last_bg_pid_none_when_all_foreground() {
        let mut table = JobTable::default();
        table.add_job(pid(5), vec![pid(5)], "fg", true);
        assert!(table.last_bg_pid().is_none());
    }

    // -----------------------------------------------------------------------
    // all_jobs sorted by id
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_jobs_sorted_by_id() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "a", false);
        table.add_job(pid(2), vec![pid(2)], "b", false);
        table.add_job(pid(3), vec![pid(3)], "c", false);

        let ids: Vec<JobId> = table.all_jobs().map(|j| j.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    // -----------------------------------------------------------------------
    // resolve_job_spec
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_job_spec_numeric() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve_job_spec("%1"), Ok(id));
    }

    #[test]
    fn test_resolve_job_spec_percent_percent() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve_job_spec("%%"), Ok(id));
    }

    #[test]
    fn test_resolve_job_spec_plus() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve_job_spec("%+"), Ok(id));
    }

    #[test]
    fn test_resolve_job_spec_minus() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let _id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        assert_eq!(table.resolve_job_spec("%-"), Ok(id1));
    }

    #[test]
    fn test_resolve_job_spec_invalid() {
        let table = JobTable::default();
        // "%99" — syntactically valid Numeric(99) but no such job
        assert_eq!(table.resolve_job_spec("%99"), Err(JobSpecError::NoSuchJob));
        // "foo" — doesn't start with '%'
        assert_eq!(table.resolve_job_spec("foo"), Err(JobSpecError::Malformed));
        // "%abc" — Prefix("abc") against empty table → NoSuchJob (previously Malformed)
        assert_eq!(table.resolve_job_spec("%abc"), Err(JobSpecError::NoSuchJob));
    }

    #[test]
    fn test_resolve_job_spec_ambiguous() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 10", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 20", false);
        assert_eq!(
            table.resolve_job_spec("%sleep"),
            Err(JobSpecError::Ambiguous)
        );
    }

    // -----------------------------------------------------------------------
    // parse_job_spec
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_current_percent() {
        assert_eq!(parse_job_spec("%%"), Ok(JobSpec::Current));
    }

    #[test]
    fn test_parse_current_plus() {
        assert_eq!(parse_job_spec("%+"), Ok(JobSpec::Current));
    }

    #[test]
    fn test_parse_previous() {
        assert_eq!(parse_job_spec("%-"), Ok(JobSpec::Previous));
    }

    #[test]
    fn test_parse_numeric() {
        assert_eq!(parse_job_spec("%1"), Ok(JobSpec::Numeric(1)));
        assert_eq!(parse_job_spec("%42"), Ok(JobSpec::Numeric(42)));
    }

    #[test]
    fn test_parse_numeric_overflow() {
        assert_eq!(
            parse_job_spec("%99999999999999999999"),
            Err(JobSpecError::Malformed)
        );
    }

    #[test]
    fn test_parse_prefix() {
        assert_eq!(parse_job_spec("%foo"), Ok(JobSpec::Prefix("foo")));
        assert_eq!(parse_job_spec("%vim"), Ok(JobSpec::Prefix("vim")));
    }

    #[test]
    fn test_parse_substring() {
        assert_eq!(parse_job_spec("%?bar"), Ok(JobSpec::Substring("bar")));
        assert_eq!(parse_job_spec("%?READ"), Ok(JobSpec::Substring("READ")));
    }

    #[test]
    fn test_parse_prefix_hyphen() {
        // "%-foo" is NOT %- followed by "foo" — it is a Prefix("-foo")
        assert_eq!(parse_job_spec("%-foo"), Ok(JobSpec::Prefix("-foo")));
    }

    #[test]
    fn test_parse_prefix_double_percent() {
        // "%%foo" is NOT Current followed by "foo" — it is Prefix("%foo")
        assert_eq!(parse_job_spec("%%foo"), Ok(JobSpec::Prefix("%foo")));
    }

    #[test]
    fn test_parse_malformed_empty() {
        assert_eq!(parse_job_spec(""), Err(JobSpecError::Malformed));
    }

    #[test]
    fn test_parse_malformed_bare_percent() {
        assert_eq!(parse_job_spec("%"), Err(JobSpecError::Malformed));
    }

    #[test]
    fn test_parse_malformed_bare_question() {
        assert_eq!(parse_job_spec("%?"), Err(JobSpecError::Malformed));
    }

    #[test]
    fn test_parse_malformed_no_percent() {
        assert_eq!(parse_job_spec("foo"), Err(JobSpecError::Malformed));
        assert_eq!(parse_job_spec("1"), Err(JobSpecError::Malformed));
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

    // -----------------------------------------------------------------------
    // format_job
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // JobTable::resolve
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_current() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve(JobSpec::Current), Ok(id));
    }

    #[test]
    fn test_resolve_current_unset() {
        let table = JobTable::default();
        assert_eq!(
            table.resolve(JobSpec::Current),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_previous() {
        let mut table = JobTable::default();
        let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
        let _id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
        assert_eq!(table.resolve(JobSpec::Previous), Ok(id1));
    }

    #[test]
    fn test_resolve_previous_unset() {
        let mut table = JobTable::default();
        let _id = table.add_job(pid(1), vec![pid(1)], "a", false);
        // Only one job added — previous is unset
        assert_eq!(
            table.resolve(JobSpec::Previous),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_numeric_hit() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "x", false);
        assert_eq!(table.resolve(JobSpec::Numeric(id)), Ok(id));
    }

    #[test]
    fn test_resolve_numeric_miss() {
        let table = JobTable::default();
        assert_eq!(
            table.resolve(JobSpec::Numeric(99)),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_prefix_single() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim README.md", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 30", false);
        assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
    }

    #[test]
    fn test_resolve_prefix_none() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 30", false);
        assert_eq!(
            table.resolve(JobSpec::Prefix("vim")),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_prefix_ambiguous() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 10", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 20", false);
        assert_eq!(
            table.resolve(JobSpec::Prefix("sleep")),
            Err(JobSpecError::Ambiguous)
        );
    }

    #[test]
    fn test_resolve_substring_single() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim README.md", false);
        table.add_job(pid(2), vec![pid(2)], "sleep 30", false);
        assert_eq!(table.resolve(JobSpec::Substring("EADME")), Ok(id));
    }

    #[test]
    fn test_resolve_substring_none() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "sleep 30", false);
        assert_eq!(
            table.resolve(JobSpec::Substring("vim")),
            Err(JobSpecError::NoSuchJob)
        );
    }

    #[test]
    fn test_resolve_substring_ambiguous() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "cat foo", false);
        table.add_job(pid(2), vec![pid(2)], "grep foo", false);
        assert_eq!(
            table.resolve(JobSpec::Substring("foo")),
            Err(JobSpecError::Ambiguous)
        );
    }

    #[test]
    fn test_resolve_prefix_matches_done_job() {
        // bash-compatible: Prefix matches all statuses, including Done
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim foo", false);
        if let Some(job) = table.get_mut(id) {
            job.status = JobStatus::Done(0);
        }
        assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
    }

    // -----------------------------------------------------------------------
    // Terminal function type signatures compile
    // -----------------------------------------------------------------------

    #[test]
    fn test_terminal_functions_compile() {
        // This test verifies the functions exist and have the correct
        // signatures.  We cannot actually call tcsetpgrp in a unit test
        // (no controlling terminal), so we just take function pointers.
        let _: fn(Pid) -> Result<(), nix::Error> = give_terminal;
        let _: fn(Pid) -> Result<(), nix::Error> = take_terminal;
    }
}
