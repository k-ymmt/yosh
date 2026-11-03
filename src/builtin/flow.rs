//! Control-flow special builtins: `exit`, `return`, `break` and
//! `continue` (POSIX XCU §2.15).

use crate::env::{FlowControl, ShellEnv};
use crate::error::{RuntimeErrorKind, ShellError};
use crate::exec::Executor;

pub fn builtin_exit(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let code = if args.is_empty() {
        // POSIX §2.12: inside a trap action, `exit` without an operand
        // uses the value $? had when the trap action started, not the
        // status of the last command in the trap action.
        executor
            .env
            .exec
            .trap_context_status
            .unwrap_or(executor.env.exec.last_exit_status)
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                // POSIX §2.8.1: a special-builtin error still terminates the
                // shell — diagnose and fall through to the normal exit
                // sequence with status 2 instead of returning to the caller.
                eprintln!("yosh: exit: {}: numeric argument required", args[0]);
                2
            }
        }
    };
    // POSIX §2.12: the EXIT trap action starts with `$?` equal to the
    // value the shell is exiting with (`trap 'echo $?' EXIT; exit 7`
    // prints 7). Store it before running any trap actions; the final
    // exit status stays `code` regardless of what the trap body runs.
    executor.env.exec.last_exit_status = code;
    executor.process_pending_signals();
    executor.execute_exit_trap();
    if executor.env.mode.is_interactive {
        executor.exit_requested = Some(code);
        Ok(code)
    } else {
        crate::exec::shell_exit(code);
    }
}

pub fn builtin_return(args: &[String], executor: &mut Executor) -> Result<i32, ShellError> {
    let env = &mut executor.env;
    if env.vars.scope_depth() <= 1 && !env.mode.in_dot_script {
        return Err(ShellError::runtime(
            RuntimeErrorKind::IoError,
            "return: can only return from a function or sourced script".to_string(),
        ));
    }
    let code = if args.is_empty() {
        env.exec.last_exit_status & 0xFF
    } else {
        match args[0].parse::<i32>() {
            Ok(n) => n & 0xFF,
            Err(_) => {
                // POSIX §2.8.1: a special-builtin error terminates a
                // non-interactive shell with status 2 (matches dash).
                eprintln!("yosh: return: {}: numeric argument required", args[0]);
                env.exec.last_exit_status = 2;
                if !env.mode.is_interactive {
                    executor.exit_requested = Some(2);
                }
                return Ok(2);
            }
        }
    };
    env.exec.flow_control = Some(FlowControl::Return(code));
    Ok(code)
}

pub fn builtin_break(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    loop_control("break", args, env, FlowControl::Break)
}

pub fn builtin_continue(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    loop_control("continue", args, env, FlowControl::Continue)
}

/// Shared body of `break [n]` / `continue [n]`: validate the loop count
/// and request `flow(n)`, with `n` clamped to the enclosing loop depth.
fn loop_control(
    name: &str,
    args: &[String],
    env: &mut ShellEnv,
    flow: fn(usize) -> FlowControl,
) -> Result<i32, ShellError> {
    if env.exec.loop_depth == 0 {
        eprintln!(
            "yosh: {}: only meaningful in a `for', `while', or `until' loop",
            name
        );
        return Ok(1);
    }
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(0) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("{}: loop count must be > 0", name),
                ));
            }
            Ok(n) => n,
            Err(_) => {
                return Err(ShellError::runtime(
                    RuntimeErrorKind::InvalidArgument,
                    format!("{}: {}: numeric argument required", name, args[0]),
                ));
            }
        }
    };
    env.exec.flow_control = Some(flow(n.min(env.exec.loop_depth)));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use crate::builtin::special::exec_special_builtin;
    use crate::exec::Executor;

    #[test]
    fn exit_builtin_sets_exit_requested_in_interactive_mode() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.mode.is_interactive = true;
        let status = exec_special_builtin("exit", &["42".to_string()], &mut executor);
        assert_eq!(status, 42);
        assert_eq!(executor.exit_requested, Some(42));
    }

    #[test]
    fn exit_builtin_uses_last_status_when_no_args() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.mode.is_interactive = true;
        executor.env.exec.last_exit_status = 7;
        exec_special_builtin("exit", &[], &mut executor);
        assert_eq!(executor.exit_requested, Some(7));
    }

    #[test]
    fn break_outside_loop_returns_one_and_no_flow_control() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("break", &[], &mut executor);
        assert_eq!(status, 1);
        assert!(executor.env.exec.flow_control.is_none());
    }

    #[test]
    fn continue_outside_loop_returns_one_and_no_flow_control() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("continue", &[], &mut executor);
        assert_eq!(status, 1);
        assert!(executor.env.exec.flow_control.is_none());
    }

    #[test]
    fn continue_n_is_clamped_to_loop_depth() {
        use crate::env::FlowControl;
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.exec.loop_depth = 1;
        let status = exec_special_builtin("continue", &["5".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert_eq!(
            executor.env.exec.flow_control,
            Some(FlowControl::Continue(1))
        );
    }

    #[test]
    fn break_n_is_clamped_to_loop_depth() {
        use crate::env::FlowControl;
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.exec.loop_depth = 2;
        let status = exec_special_builtin("break", &["7".to_string()], &mut executor);
        assert_eq!(status, 0);
        assert_eq!(executor.env.exec.flow_control, Some(FlowControl::Break(2)));
    }
}
