//! AND-OR lists (`a && b || c`): short-circuit evaluation, the `set -e`
//! exemption of their non-final pipelines and the trap actions run
//! between them.

use crate::exec::Executor;
use crate::parser::ast::{AndOrList, AndOrOp, Pipeline};

impl Executor {
    /// Execute an AND-OR list.
    pub(crate) fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let has_rest = !and_or.rest.is_empty();

        // POSIX §2.15 set: the final status is exempt from `set -e` when the
        // pipeline that produced it began with `!`, or when it came from a
        // non-final component of the list (short-circuit). The flag is set
        // AFTER each pipeline runs (nested lists inside the pipeline set it
        // for themselves) so the value left when we return describes the
        // status we return.
        let first_exempt = and_or.first.negated || has_rest;
        let mut status = self.exec_pipeline_errexit(&and_or.first, first_exempt);

        if self.should_unwind() || self.and_or_step_traps(has_rest) {
            return status;
        }

        for (i, (op, pipeline)) in and_or.rest.iter().enumerate() {
            let is_last = i == and_or.rest.len() - 1;
            let should_run = match op {
                AndOrOp::And => status == 0,
                AndOrOp::Or => status != 0,
            };
            if !should_run {
                continue;
            }

            let exempt = pipeline.negated || !is_last;
            status = self.exec_pipeline_errexit(pipeline, exempt);

            if self.should_unwind() || self.and_or_step_traps(!is_last) {
                break;
            }
        }

        self.env.exec.last_exit_status = status;
        status
    }

    /// Pending trap actions between the pipelines of an AND-OR list: like
    /// between list items, a trapped signal's action runs after the
    /// current command (POSIX §2.11), so `trap 'echo T' USR1;
    /// kill -USR1 $$ && echo yes` prints T before yes (dash and bash
    /// agree). Skipped after the last pipeline, whose list item drains
    /// them. Returns true when a trap action asked the shell to exit.
    fn and_or_step_traps(&mut self, more: bool) -> bool {
        if !more {
            return false;
        }
        self.process_pending_signals();
        self.exit_requested.is_some()
    }

    /// Run one pipeline of an AND-OR list, maintaining
    /// `errexit_exempt_status`: cleared before the run (so a stale value
    /// from a previous list cannot leak in), forced on afterwards when this
    /// pipeline is itself exempt, and otherwise left as the pipeline's body
    /// set it — an in-process compound (brace group, loop, case) whose final
    /// status came from an exempt pipeline propagates that exemption
    /// (matches bash/dash; functions and subshells do not propagate).
    fn exec_pipeline_errexit(&mut self, pipeline: &Pipeline, exempt: bool) -> i32 {
        self.errexit_exempt_status = false;
        let status = if exempt {
            self.with_errexit_suppressed(|e| e.exec_pipeline(pipeline))
        } else {
            self.exec_pipeline(pipeline)
        };
        if exempt {
            self.errexit_exempt_status = true;
        }
        status
    }
}
