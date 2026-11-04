//! Signal handling: the signal tables, self-pipe delivery, disposition
//! management for the shell and its forked children, and job-control
//! signal setup.
//!
//! * `table` — name/number tables, the handled set, default actions.
//! * `self_pipe` — the async-signal-safe handler, its self-pipe, and
//!   the pending-signal flags.
//! * `entry` — signals inherited as SIG_IGN at shell entry (§2.12).
//! * `disposition` — SIG_IGN/SIG_DFL primitives and trap dispositions.
//! * `child` — disposition reset for forked children.
//! * `job_control` — monitor-mode setup and the foreground wait.

mod child;
mod disposition;
mod entry;
mod job_control;
mod self_pipe;
mod table;

pub use child::{
    reset_child_signals, reset_shell_child_signals, setup_background_shell_child_signals,
    setup_foreground_child_signals, setup_foreground_shell_child_signals, spawn_default_sigset,
};
pub use disposition::{
    TrapDisposition, apply_trap_disposition, assume_all_dispositions_changed, default_signal,
    note_disposition_change,
};
pub use entry::{ignored_on_entry_set_opt, is_ignored_on_entry};
pub use job_control::{
    init_job_control_signals, reset_job_control_signals, try_enable_monitor_mode,
    wait_until_foreground,
};
pub use self_pipe::{
    drain_pending_signals, has_pending_exit_signal, init_signal_handling, requeue_signals,
    self_pipe_read_fd, set_interactive_shell, try_self_pipe_read_fd,
};
pub use table::{
    DefaultAction, SIGNAL_TABLE, default_action, signal_name_to_number, signal_number_to_name,
};
#[allow(unused_imports)] // public API the binary's own module tree does not reference
pub use {disposition::ignore_signal, entry::ignored_on_entry_set, table::HANDLED_SIGNALS};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_control_signal_functions_exist() {
        let _ = init_job_control_signals as fn();
        let _ = reset_job_control_signals as fn();
        let _ = setup_foreground_child_signals as fn(&[i32]);
        let _ = setup_background_shell_child_signals as fn(&[i32]);
    }

    #[test]
    fn test_reset_job_control_signals_after_init() {
        init_signal_handling();
        init_job_control_signals();
        reset_job_control_signals();
        // No panic = success
    }
}
