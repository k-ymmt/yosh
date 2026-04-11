/// Flow control signals for break, continue, and return.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowControl {
    Break(usize),
    Continue(usize),
    Return(i32),
}

/// Execution-related state.
#[derive(Debug, Clone)]
pub struct ExecState {
    pub last_exit_status: i32,
    pub flow_control: Option<FlowControl>,
}
