use crate::env::FlowControl;
use crate::parser::ast::FunctionDef;

use super::Executor;

/// RAII guard that undoes the per-call scope state (positional-param
/// scope, indirection level, loop depth) when it drops. Replaces the
/// previous per-call `catch_unwind`: `Drop` runs on both the normal
/// return path and a panic unwind, without paying the unwind-catch
/// machinery on every call.
struct ScopeGuard<'a> {
    exec: &'a mut Executor,
    saved_loop_depth: usize,
}

impl Drop for ScopeGuard<'_> {
    fn drop(&mut self) {
        self.exec.env.exec.loop_depth = self.saved_loop_depth;
        self.exec.env.exec.indirection_level -= 1;
        self.exec.env.vars.pop_scope();
    }
}

impl Executor {
    /// Invoke a function: push a new scope for positional params, execute body.
    /// A Drop guard (`ScopeGuard`) ensures the scope is popped on every
    /// exit path, including panic unwinds.
    pub(crate) fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
        self.env.vars.push_scope(args.to_vec());
        self.env.exec.indirection_level += 1;
        // POSIX 2024: break/continue are lexically contained — a function
        // body has no enclosing loop even when the call site is inside one.
        let saved_loop_depth = std::mem::take(&mut self.env.exec.loop_depth);

        let compound_result = {
            let guard = ScopeGuard {
                exec: self,
                saved_loop_depth,
            };
            guard
                .exec
                .exec_compound_command(&func_def.body, &func_def.redirects)
            // guard drops here (or during an unwind), restoring
            // loop_depth / indirection_level and popping the scope.
        };

        // A function's nonzero return is subject to `set -e` at the call
        // site even when the body's final pipeline began with `!`
        // (bash/dash: the exemption does not cross the call boundary).
        self.clear_errexit_exempt();

        let status = match compound_result {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", e);
                let code = e.exit_code();
                self.env.exec.last_exit_status = code;
                code
            }
        };

        // Handle return flow control
        let final_status = match self.env.exec.flow_control.take() {
            Some(FlowControl::Return(s)) => s,
            Some(other) => {
                self.env.exec.flow_control = Some(other);
                status
            }
            None => status,
        };

        // Drain async signal traps at the call boundary so a trap installed
        // inside a long-running function fires when the function returns,
        // not only after the next top-level command completes. Runs BEFORE
        // last_exit_status is finalized so the trap action cannot clobber
        // the function's `$?`. Fast-path cost is one atomic store plus one
        // non-blocking self-pipe read(2) (see signal::drain_pending_signals).
        self.process_pending_signals();

        self.env.exec.last_exit_status = final_status;
        final_status
    }
}
