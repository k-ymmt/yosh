//! POSIX §3.204 job-control job specifier parsing and resolution.
//!
//! `parse_job_spec` is a pure parser; `JobTable::resolve_job_spec` and
//! `JobTable::resolve` look the parsed spec up against the current
//! job table state. `resolve_by` is the shared scan-and-disambiguate
//! helper for Prefix/Substring matching.

use super::JobId;

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

impl super::JobTable {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::jobs::JobTable;
    use nix::unistd::Pid;

    fn pid(n: i32) -> Pid {
        Pid::from_raw(n)
    }

    // resolve_job_spec tests

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

    // parse_job_spec tests

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

    // JobTable::resolve tests

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
        use crate::env::jobs::JobStatus;
        let mut table = JobTable::default();
        let id = table.add_job(pid(1), vec![pid(1)], "vim foo", false);
        if let Some(job) = table.get_mut(id) {
            job.status = JobStatus::Done(0);
        }
        assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
    }
}
