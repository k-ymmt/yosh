use crate::env::FlowControl;
use crate::parser::ast::FunctionDef;

use super::Executor;

impl Executor {
    /// Invoke a function: push a new scope for positional params, execute body.
    /// Uses catch_unwind for panic safety to ensure scope is always popped.
    pub(crate) fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
        self.env.vars.push_scope(args.to_vec());
        self.env.exec.indirection_level += 1;
        // POSIX 2024: break/continue are lexically contained — a function
        // body has no enclosing loop even when the call site is inside one.
        let saved_loop_depth = std::mem::take(&mut self.env.exec.loop_depth);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.exec_compound_command(&func_def.body, &func_def.redirects)
        }));

        self.env.exec.loop_depth = saved_loop_depth;
        self.env.exec.indirection_level -= 1;
        self.env.vars.pop_scope();

        let compound_result = match result {
            Ok(s) => s,
            Err(payload) => std::panic::resume_unwind(payload),
        };

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

        self.env.exec.last_exit_status = final_status;
        final_status
    }
}
