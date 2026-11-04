//! The `jobs` builtin and job-status notifications.

use super::super::Executor;
use super::strip_job_spec_prefix;
use crate::env::jobs::JobSpecError;
use crate::error::ShellError;

/// Parsed form of a `jobs [-l|-p] [--] [job_spec...]` invocation.
#[derive(Debug)]
struct JobsOpts {
    long_format: bool,
    pgid_only: bool,
    operands: Vec<String>,
}

/// Parse `jobs` flags + operands. Returns `Err(message)` on unknown
/// option; `message` is already prefixed (e.g., `"jobs: -x: invalid option"`)
/// for the caller to write to stderr verbatim.
fn parse_options(args: &[String]) -> Result<JobsOpts, String> {
    let mut long_format = false;
    let mut pgid_only = false;
    let mut idx = 0;

    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'l' => long_format = true,
                'p' => pgid_only = true,
                other => return Err(format!("jobs: -{}: invalid option", other)),
            }
        }
        idx += 1;
    }

    let operands = args[idx..].to_vec();
    Ok(JobsOpts {
        long_format,
        pgid_only,
        operands,
    })
}

impl Executor {
    pub(in crate::exec) fn builtin_jobs(&mut self, args: &[String]) -> Result<i32, ShellError> {
        let opts = match parse_options(args) {
            Ok(o) => o,
            Err(msg) => {
                crate::byteenc::write_stderr_decoded_line(&format!("yosh: {}", msg));
                return Ok(1);
            }
        };

        // Report current statuses: a job that finished since the last
        // complete command (`sleep 0 & sleep 1; jobs` on one line) is
        // Done, not Running (dash reaps here too).
        self.reap_zombies();

        // Decide which job IDs to print.
        let mut exit_status = 0;
        let job_ids: Vec<crate::env::jobs::JobId> = if opts.operands.is_empty() {
            self.env.process.jobs.all_jobs().map(|j| j.id).collect()
        } else {
            let mut resolved = Vec::with_capacity(opts.operands.len());
            for spec in &opts.operands {
                match self.env.process.jobs.resolve_job_spec(spec) {
                    Ok(id) => resolved.push(id),
                    Err(JobSpecError::Ambiguous) => {
                        let display = strip_job_spec_prefix(spec);
                        crate::byteenc::write_stderr_decoded_line(&format!(
                            "yosh: jobs: {}: ambiguous job spec",
                            display
                        ));
                        exit_status = 1;
                    }
                    Err(_) => {
                        crate::byteenc::write_stderr_decoded_line(&format!(
                            "yosh: jobs: {}: no such job",
                            spec
                        ));
                        exit_status = 1;
                    }
                }
            }
            resolved
        };

        let jobs = &self.env.process.jobs;
        let lines = job_ids.iter().filter_map(|id| {
            if opts.pgid_only {
                jobs.get(*id).map(|job| job.pgid.as_raw().to_string())
            } else if opts.long_format {
                jobs.format_job_long(*id)
            } else {
                jobs.format_job(*id)
            }
        });
        if crate::builtin::output::write_lines_or_report("jobs", lines) != 0 {
            exit_status = 1;
        }

        // POSIX XCU jobs: a finished job whose status has been reported
        // is removed from the job list, so its number becomes free (bash
        // and dash both drop `[1] Done` after printing it once). Only
        // the jobs actually listed count as reported.
        let pending = self.env.process.jobs.pending_notifications();
        for id in pending.into_iter().filter(|id| job_ids.contains(id)) {
            self.env.process.jobs.mark_notified(id);
        }
        self.env.process.jobs.cleanup_notified();

        Ok(exit_status)
    }

    /// Display pending job notifications and clean up completed jobs.
    pub(crate) fn display_job_notifications(&mut self) {
        let pending = self.env.process.jobs.pending_notifications();
        for id in &pending {
            if let Some(line) = self.env.process.jobs.format_job(*id) {
                crate::byteenc::write_stderr_decoded_line(&line);
            }
            self.env.process.jobs.mark_notified(*id);
        }
        self.env.process.jobs.cleanup_notified();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_options_recognizes_long_flag() {
        let args = vec!["-l".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert!(!opts.pgid_only);
        assert_eq!(opts.operands, Vec::<String>::new());
    }

    #[test]
    fn parse_options_recognizes_pgid_flag() {
        let args = vec!["-p".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(!opts.long_format);
        assert!(opts.pgid_only);
    }

    #[test]
    fn parse_options_clustered_flags() {
        let args = vec!["-lp".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert!(opts.pgid_only);
    }

    #[test]
    fn parse_options_double_dash_ends_flags() {
        let args = vec!["--".to_string(), "%1".to_string()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.operands, vec!["%1".to_string()]);
    }

    #[test]
    fn parse_options_rejects_unknown_flag() {
        let args = vec!["-x".to_string()];
        let err = parse_options(&args).unwrap_err();
        assert!(err.contains("jobs:") && err.contains("-x"));
    }

    #[test]
    fn parse_options_collects_operands_after_flags() {
        let args = vec!["-l".to_string(), "%1".to_string(), "%2".to_string()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.long_format);
        assert_eq!(opts.operands, vec!["%1".to_string(), "%2".to_string()]);
    }
}
