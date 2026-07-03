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
    /// Number of nested function-call and dot-script invocations currently
    /// on the stack. Used only to replicate the first character of PS4 in
    /// `set -x` trace output (POSIX "levels of indirection"). Subshells and
    /// command substitutions are NOT counted.
    pub indirection_level: usize,
    /// Source line of the simple/compound command currently executing.
    /// Backs `$LINENO`. Stored as a plain integer (not a `VarStore` entry)
    /// so that the per-command write does not invalidate the exported-
    /// environ cache: `$LINENO` is intercepted directly in
    /// `expand::param` rather than surfaced as a real shell variable
    /// (matches bash/dash: not listed by `set`, not exportable, and a
    /// user assignment/readonly does not "stick" — the next command
    /// overwrites it).
    ///
    /// Edge case: `export LINENO` / `readonly LINENO` (bare, no `=`) still
    /// go through `VarStore::export` / `set_readonly`, which create a
    /// phantom `VarStore` entry (empty value) when the name isn't already
    /// present — since `$LINENO` is never actually written there, this
    /// entry then sits inert but exported/readonly. That phantom entry
    /// exports as `LINENO=` (empty) to child environments, while `$LINENO`
    /// itself keeps reading the live intercept (i.e. the exported child
    /// sees a stale empty value, not the shell's current line). bash, by
    /// contrast, exports the live numeric value. This divergence is
    /// accepted rather than fixed.
    pub lineno: usize,
}
