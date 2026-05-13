/// Flow control signals for break, continue, and return.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}

/// Execution-related state.
#[derive(Debug, Clone, Default)]
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
    /// Number of currently-executing loop bodies (for / while / until).
    /// Used by `break` / `continue` to detect out-of-loop usage and to
    /// clamp `n` against the outermost loop (POSIX §2.14.1 / §2.14.5).
    pub loop_depth: usize,
}
