use nix::unistd::Pid;
use std::collections::{HashMap, VecDeque};

mod format;
mod model;
mod notification;
mod spec;
mod terminal;

pub use model::{Job, JobId, JobStatus};
pub use spec::JobSpecError;
pub use terminal::{give_terminal, take_terminal};
pub(crate) use terminal::terminal_fd;

// ---------------------------------------------------------------------------
// JobTable
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct JobTable {
    jobs: HashMap<JobId, Job>,
    next_id: JobId,
    current: Option<JobId>,
    previous: Option<JobId>,
    /// Termios snapshot captured once at interactive REPL startup. Used to
    /// restore the shell's terminal state after every foreground wait
    /// completion. `None` in non-interactive / non-monitor mode.
    shell_tmodes: Option<nix::sys::termios::Termios>,
    /// Wait-style exit statuses (`code` or `128+sig`) of background jobs
    /// that were reaped, notified, and removed from the table. POSIX XCU
    /// `wait` requires known `$!` pids to stay waitable after the
    /// interactive notification pass drops the job; bash keeps such
    /// statuses (non-consuming) until a no-operand `wait` discards them
    /// (empirical, 2026-08-25). FIFO order for CHILD_MAX-bounded
    /// eviction of the oldest entries.
    reaped_statuses: VecDeque<(Pid, i32)>,
    /// Pgid of the job most recently placed in the background — the
    /// value of `$!`. Stored rather than derived from the live table so
    /// `$!` survives the job's removal (bash keeps `$!` after the
    /// notification pass drops the job; empirical 2026-08-25). Set by
    /// `add_job` for background jobs and by `bg` (bash: `$!` is the job
    /// most recently backgrounded, whether via `&` or `bg`).
    last_async_pid: Option<Pid>,
}

/// Retention bound for `reaped_statuses`: POSIX requires remembering at
/// least {CHILD_MAX} asynchronous job statuses. Falls back to 1024 when
/// sysconf reports no value.
fn reaped_cap() -> usize {
    nix::unistd::sysconf(nix::unistd::SysconfVar::CHILD_MAX)
        .ok()
        .flatten()
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(1024)
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

        // A reused pid now names a live process: its predecessor's
        // remembered status must not be reachable once this job later
        // leaves the table through a path that does not re-record it
        // (e.g. wait_for_foreground_job after `fg`).
        self.reaped_statuses
            .retain(|(p, _)| !pids.contains(p) && *p != pgid);

        let job = Job {
            id,
            pgid,
            pids,
            command: command.into(),
            status: JobStatus::Running,
            notified: false,
            foreground,
            saved_tmodes: None,
        };

        self.jobs.insert(id, job);

        // The new job becomes current; old current becomes previous.
        self.previous = self.current;
        self.current = Some(id);

        if !foreground {
            self.last_async_pid = Some(pgid);
        }

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
            notification::reset_after_status_change(job);
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

    /// Return the pgid of the job most recently placed in the
    /// background (`$!`). Survives the job's removal from the table
    /// (notification cleanup, foreground completion after `fg`).
    pub fn last_bg_pid(&self) -> Option<Pid> {
        self.last_async_pid
    }

    /// Record `pgid` as the most recently backgrounded job (`$!`).
    /// Called by `add_job` for background jobs; `bg` calls it directly
    /// when resuming a stopped job (bash parity).
    pub fn set_last_bg_pid(&mut self, pgid: Pid) {
        self.last_async_pid = Some(pgid);
    }

    // -----------------------------------------------------------------------
    // Reaped-status retention (POSIX XCU wait: known pids stay waitable)
    // -----------------------------------------------------------------------

    /// Remember the wait-style exit status of a reaped pid whose job is
    /// being removed from the table, so a later `wait <pid>` can still
    /// report it. Re-recording a pid (pid reuse) replaces the old entry.
    pub fn record_reaped(&mut self, pid: Pid, status: i32) {
        self.record_reaped_bounded(pid, status, reaped_cap());
    }

    fn record_reaped_bounded(&mut self, pid: Pid, status: i32, cap: usize) {
        self.reaped_statuses.retain(|(p, _)| *p != pid);
        self.reaped_statuses.push_back((pid, status));
        while self.reaped_statuses.len() > cap {
            self.reaped_statuses.pop_front();
        }
    }

    /// Look up the remembered status of a reaped-and-forgotten pid.
    /// Non-consuming: repeated `wait <pid>` keeps returning the status
    /// (bash behavior) until `clear_reaped` discards it.
    pub fn reaped_status(&self, pid: Pid) -> Option<i32> {
        self.reaped_statuses
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|&(_, status)| status)
    }

    /// Discard all remembered statuses. POSIX XCU wait: a no-operand
    /// `wait` may discard known process IDs once it completes; bash does
    /// (`wait; wait $p` reports "not a child", empirical 2026-08-25).
    pub fn clear_reaped(&mut self) {
        self.reaped_statuses.clear();
    }

    /// Reset inherited wait state in a forked subshell child. The
    /// parent's reaped-status map and its already-terminal table jobs
    /// are not the child's children and must not be waitable there
    /// (bash/dash: `(wait $p)` on a finished parent job reports "not a
    /// child of this shell", 127 — empirical 2026-08-25). Running jobs
    /// stay listed: they resolve through waitpid (which correctly
    /// reports ECHILD in the child), and keeping them preserves
    /// `$(jobs)` output parity with bash for running jobs.
    pub fn reset_for_subshell(&mut self) {
        self.clear_reaped();
        let terminal: Vec<JobId> = self
            .jobs
            .values()
            .filter(|j| j.status.is_terminal())
            .map(|j| j.id)
            .collect();
        for id in terminal {
            self.remove_job(id);
        }
    }

    /// Iterate over all jobs sorted by id (ascending).
    pub fn all_jobs(&self) -> impl Iterator<Item = &Job> {
        let mut ids: Vec<JobId> = self.jobs.keys().copied().collect();
        ids.sort();
        // Collect into Vec so we own the sorted order.
        let sorted: Vec<&Job> = ids.iter().map(|id| &self.jobs[id]).collect();
        sorted.into_iter()
    }
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

    // -----------------------------------------------------------------------
    // Reaped-status retention
    // -----------------------------------------------------------------------

    #[test]
    fn test_record_reaped_lookup_is_non_consuming() {
        let mut table = JobTable::default();
        table.record_reaped(pid(100), 7);
        assert_eq!(table.reaped_status(pid(100)), Some(7));
        // bash keeps the status across repeated waits.
        assert_eq!(table.reaped_status(pid(100)), Some(7));
        assert_eq!(table.reaped_status(pid(999)), None);
    }

    #[test]
    fn test_record_reaped_rerecord_replaces_old_entry() {
        let mut table = JobTable::default();
        table.record_reaped(pid(100), 7);
        table.record_reaped(pid(100), 3);
        assert_eq!(table.reaped_status(pid(100)), Some(3));
        assert_eq!(table.reaped_statuses.len(), 1);
    }

    #[test]
    fn test_record_reaped_evicts_oldest_beyond_cap() {
        let mut table = JobTable::default();
        table.record_reaped_bounded(pid(1), 1, 2);
        table.record_reaped_bounded(pid(2), 2, 2);
        table.record_reaped_bounded(pid(3), 3, 2);
        assert_eq!(table.reaped_status(pid(1)), None, "oldest must be evicted");
        assert_eq!(table.reaped_status(pid(2)), Some(2));
        assert_eq!(table.reaped_status(pid(3)), Some(3));
    }

    #[test]
    fn test_last_bg_pid_survives_job_removal() {
        let mut table = JobTable::default();
        let id = table.add_job(pid(100), vec![pid(100)], "bg job", false);
        table.update_status(pid(100), JobStatus::Done(7));
        table.mark_notified(id);
        table.cleanup_notified();

        // POSIX: `$!` expands to the most recent background command's
        // pid; it must not become unset because the notification pass
        // dropped the job from the table (bash parity).
        assert_eq!(table.last_bg_pid(), Some(pid(100)));
    }

    #[test]
    fn test_last_bg_pid_survives_reset_for_subshell() {
        let mut table = JobTable::default();
        table.add_job(pid(100), vec![pid(100)], "bg job", false);
        table.reset_for_subshell();
        // bash: `sleep 5 & (echo $!)` prints the pid — subshells
        // inherit `$!` even though the job is not their child.
        assert_eq!(table.last_bg_pid(), Some(pid(100)));
    }

    #[test]
    fn test_set_last_bg_pid_updates_dollar_bang() {
        let mut table = JobTable::default();
        table.add_job(pid(100), vec![pid(100)], "old bg", false);
        // `bg` on a stopped job records that job as `$!` (bash parity).
        table.set_last_bg_pid(pid(200));
        assert_eq!(table.last_bg_pid(), Some(pid(200)));
    }

    #[test]
    fn test_add_job_invalidates_stale_reaped_entry_for_reused_pid() {
        let mut table = JobTable::default();
        table.record_reaped(pid(100), 7);
        // The OS reuses pid 100 for a new background job: the old
        // remembered status must not survive (the new job may later
        // leave the table via a path that does not re-record it).
        table.add_job(pid(100), vec![pid(100)], "new job", false);
        assert_eq!(table.reaped_status(pid(100)), None);
    }

    #[test]
    fn test_reset_for_subshell_forgets_map_and_terminal_jobs() {
        let mut table = JobTable::default();
        table.record_reaped(pid(50), 7);
        let done = table.add_job(pid(100), vec![pid(100)], "done job", false);
        table.update_status(pid(100), JobStatus::Done(7));
        let running = table.add_job(pid(200), vec![pid(200)], "running job", false);

        table.reset_for_subshell();

        assert_eq!(table.reaped_status(pid(50)), None, "map must be cleared");
        assert!(
            table.get(done).is_none(),
            "terminal jobs must leave the table (not waitable in a subshell)",
        );
        assert!(
            table.get(running).is_some(),
            "running jobs stay listed for $(jobs) parity",
        );
    }

    #[test]
    fn test_clear_reaped_discards_all() {
        let mut table = JobTable::default();
        table.record_reaped(pid(100), 7);
        table.record_reaped(pid(200), 128 + 15);
        table.clear_reaped();
        assert_eq!(table.reaped_status(pid(100)), None);
        assert_eq!(table.reaped_status(pid(200)), None);
    }

    #[test]
    fn test_all_jobs_sorted_by_id() {
        let mut table = JobTable::default();
        table.add_job(pid(1), vec![pid(1)], "a", false);
        table.add_job(pid(2), vec![pid(2)], "b", false);
        table.add_job(pid(3), vec![pid(3)], "c", false);

        let ids: Vec<JobId> = table.all_jobs().map(|j| j.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }
}
