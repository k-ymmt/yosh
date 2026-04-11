use crate::env::FlowControl;
use crate::parser::ast::FunctionDef;

use super::Executor;

impl Executor {
    /// Invoke a function: push a new scope for positional params, execute body.
    /// Uses catch_unwind for panic safety to ensure scope is always popped.
    pub(crate) fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
        self.env.vars.push_scope(args.to_vec());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.exec_compound_command(&func_def.body, &func_def.redirects)
        }));

        self.env.vars.pop_scope();

        let status = match result {
            Ok(s) => s,
            Err(payload) => std::panic::resume_unwind(payload),
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
