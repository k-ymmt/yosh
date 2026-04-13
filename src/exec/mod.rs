pub mod command;
mod compound;
mod function;
pub mod pipeline;
pub mod redirect;
mod simple;

use nix::unistd::{fork, ForkResult};

use crate::env::ShellEnv;
use crate::plugin::PluginManager;
use crate::signal;
use crate::parser::ast::{
    AndOrList, AndOrOp, Command, CompleteCommand, Program, SeparatorOp,
};

pub struct Executor {
    pub env: ShellEnv,
    pub plugins: PluginManager,
    errexit_suppressed_depth: usize,
}

impl Executor {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        Executor {
            env: ShellEnv::new(shell_name, args),
            plugins: PluginManager::new(),
            errexit_suppressed_depth: 0,
        }
    }

    /// Create an Executor from an existing ShellEnv (e.g. for subshell/command substitution).
    pub fn from_env(env: ShellEnv) -> Self {
        Executor {
            env,
            plugins: PluginManager::new(),
            errexit_suppressed_depth: 0,
        }
    }

    /// Load plugins from the default config path (~/.config/kish/plugins.toml).
    pub fn load_plugins(&mut self) {
        let config_path = plugin_config_path();
        self.plugins.load_from_config(&config_path, &mut self.env);
    }

    /// Execute closure within errexit-suppressed context.
    pub fn with_errexit_suppressed<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.errexit_suppressed_depth += 1;
        let result = f(self);
        self.errexit_suppressed_depth -= 1;
        result
    }

    /// Check if errexit is active and not suppressed.
    pub fn should_errexit(&self) -> bool {
        self.env.mode.options.errexit && self.errexit_suppressed_depth == 0
    }

    /// Errexit check after command execution.
    pub fn check_errexit(&mut self, status: i32) {
        if status != 0 && self.should_errexit() {
            self.execute_exit_trap();
            std::process::exit(status);
        }
    }

    /// Execute the EXIT trap if set.
    pub fn execute_exit_trap(&mut self) {
        if let Some(crate::env::TrapAction::Command(cmd)) = self.env.traps.exit_trap.take() {
            self.with_errexit_suppressed(|exec| {
                exec.eval_string(&cmd);
            });
        }
    }

    /// Process any pending signals from the self-pipe.
    pub fn process_pending_signals(&mut self) {
        let signals = signal::drain_pending_signals();
        for sig in signals {
            // SIGCHLD default action is to ignore (just reap children).
            // We must not route it through handle_default_signal which
            // exits the shell.  Reaping is already handled by
            // reap_zombies() in the interactive loop.
            if sig == libc::SIGCHLD {
                match self.env.traps.get_signal_trap(sig).cloned() {
                    Some(crate::env::TrapAction::Command(cmd)) => {
                        self.with_errexit_suppressed(|exec| {
                            exec.eval_string(&cmd);
                        });
                    }
                    // Default and Ignore: just ignore SIGCHLD (reaping
                    // is done elsewhere).
                    _ => {}
                }
                continue;
            }

            match self.env.traps.get_signal_trap(sig).cloned() {
                Some(crate::env::TrapAction::Command(cmd)) => {
                    self.with_errexit_suppressed(|exec| {
                        exec.eval_string(&cmd);
                    });
                }
                Some(crate::env::TrapAction::Ignore) => {}
                Some(crate::env::TrapAction::Default) | None => {
                    self.handle_default_signal(sig);
                }
            }
        }
    }

    /// Handle a signal with default behavior (terminate).
    fn handle_default_signal(&mut self, sig: i32) {
        self.execute_exit_trap();
        std::process::exit(128 + sig);
    }

    /// Evaluate a string as shell commands (used by trap actions and eval).
    pub fn eval_string(&mut self, input: &str) {
        if let Ok(program) =
            crate::parser::Parser::new_with_aliases(input, &self.env.aliases).parse_program()
        {
            self.exec_program(&program);
        }
    }

    /// Print the line if verbose mode is enabled.
    pub fn verbose_print(&self, line: &str) {
        if self.env.mode.options.verbose {
            eprintln!("{}", line);
        }
    }

    /// Dispatch a `Command` to the appropriate execution path.
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
        if self.env.mode.options.noexec {
            return 0;
        }
        match cmd {
            Command::Simple(simple) => self.exec_simple_command(simple),
            Command::Compound(compound, redirects) => {
                self.exec_compound_command(compound, redirects)
            }
            Command::FunctionDef(func_def) => {
                self.env
                    .functions
                    .insert(func_def.name.clone(), func_def.clone());
                0
            }
        }
    }

    /// Execute an AND-OR list.
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let has_rest = !and_or.rest.is_empty();

        let mut status = if and_or.first.negated || has_rest {
            self.with_errexit_suppressed(|e| e.exec_pipeline(&and_or.first))
        } else {
            self.exec_pipeline(&and_or.first)
        };

        if self.env.exec.flow_control.is_some() {
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

            status = if pipeline.negated || !is_last {
                self.with_errexit_suppressed(|e| e.exec_pipeline(pipeline))
            } else {
                self.exec_pipeline(pipeline)
            };

            if self.env.exec.flow_control.is_some() {
                break;
            }
        }

        self.env.exec.last_exit_status = status;
        status
    }

    /// Reap any zombie background children without blocking.
    pub(crate) fn reap_zombies(&mut self) {
        use crate::env::jobs::JobStatus;
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG | nix::sys::wait::WaitPidFlag::WUNTRACED),
            ) {
                Ok(nix::sys::wait::WaitStatus::Exited(pid, code)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Done(code));
                }
                Ok(nix::sys::wait::WaitStatus::Signaled(pid, sig, _)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Terminated(sig as i32));
                }
                Ok(nix::sys::wait::WaitStatus::Stopped(pid, sig)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Stopped(sig as i32));
                }
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    /// Execute a command asynchronously (background with &).
    fn exec_async(&mut self, and_or: &AndOrList) -> i32 {
        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                // Set process group BEFORE signal setup to ensure proper isolation.
                let pid = nix::unistd::getpid();
                nix::unistd::setpgid(pid, pid).ok();

                let ignored = self.env.traps.ignored_signals();
                self.env.traps.reset_non_ignored();
                if self.env.mode.options.monitor {
                    signal::setup_background_child_signals(&ignored);
                } else {
                    signal::reset_child_signals(&ignored);
                }

                // Note: we do NOT call ignore_signal(SIGINT/SIGQUIT) here.
                // setpgid already isolates this process from keyboard signals,
                // and reset_child_signals would undo the ignore anyway.
                let status = self.exec_and_or(and_or);
                std::process::exit(status);
            }
            Ok(ForkResult::Parent { child }) => {
                nix::unistd::setpgid(child, child).ok();
                let job_id = self.env.process.jobs.add_job(child, vec![child], "(background)", false);
                eprintln!("[{}] {}", job_id, child.as_raw());
                0
            }
        }
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // Reap any finished background children before forking new ones
        self.reap_zombies();

        // -b flag: immediate job notification
        if self.env.mode.options.notify {
            self.display_job_notifications();
        }

        let mut status = 0;

        for (and_or, separator) in &cmd.items {
            if separator == &Some(SeparatorOp::Amp) {
                status = self.exec_async(and_or);
            } else {
                // Sequential execution
                status = self.exec_and_or(and_or);
            }
            if self.env.exec.flow_control.is_some() {
                break;
            }
            self.check_errexit(status);
        }

        self.env.exec.last_exit_status = status;
        status
    }

    /// Execute a program (sequence of complete commands).
    pub fn exec_program(&mut self, program: &Program) -> i32 {
        let mut status = 0;
        for cmd in &program.commands {
            status = self.exec_complete_command(cmd);
        }
        self.env.exec.last_exit_status = status;
        status
    }

    /// POSIX wait builtin: wait for background jobs.
    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;
        use crate::env::jobs::JobStatus;

        let target_pids: Vec<Pid> = if args.is_empty() {
            self.env
                .process.jobs
                .all_jobs()
                .filter(|j| j.status == JobStatus::Running)
                .map(|j| j.pgid)
                .collect()
        } else {
            let mut pids = Vec::new();
            for arg in args {
                if let Some(job_id) = self.env.process.jobs.resolve_job_spec(arg) {
                    if let Some(job) = self.env.process.jobs.get(job_id) {
                        pids.push(job.pgid);
                    } else {
                        eprintln!("kish: wait: {}: no such job", arg);
                        return 127;
                    }
                } else {
                    match arg.parse::<i32>() {
                        Ok(n) => pids.push(Pid::from_raw(n)),
                        Err(_) => {
                            eprintln!("kish: wait: {}: not a pid or valid job spec", arg);
                            return 2;
                        }
                    }
                }
            }
            pids
        };

        if target_pids.is_empty() {
            return self.env.exec.last_exit_status;
        }

        let mut last_status = 0;

        for pid in &target_pids {
            // Check if already completed in jobs table
            let already_done = self.env.process.jobs.all_jobs().find(|j| j.pgid == *pid).and_then(|j| {
                match j.status {
                    JobStatus::Done(code) => Some(code),
                    JobStatus::Terminated(sig) => Some(128 + sig),
                    _ => None,
                }
            });
            if let Some(s) = already_done {
                last_status = s;
                continue;
            }

            loop {
                match waitpid(*pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::Exited(p, code)) => {
                        self.env.process.jobs.update_status(p, JobStatus::Done(code));
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::Signaled(p, sig, _)) => {
                        let code = 128 + sig as i32;
                        self.env.process.jobs.update_status(p, JobStatus::Terminated(sig as i32));
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::StillAlive) => {
                        // Poll self-pipe with a short timeout so we also notice
                        // SIGCHLD (which is not written to the self-pipe).
                        let pipe_fd = signal::self_pipe_read_fd();
                        let mut fds = [nix::poll::PollFd::new(
                            unsafe { std::os::fd::BorrowedFd::borrow_raw(pipe_fd) },
                            nix::poll::PollFlags::POLLIN,
                        )];
                        match nix::poll::poll(&mut fds, nix::poll::PollTimeout::from(50u16)) {
                            Ok(_)
                                if fds[0]
                                    .revents()
                                    .is_some_and(|r| r.contains(nix::poll::PollFlags::POLLIN)) =>
                            {
                                let signals = signal::drain_pending_signals();
                                if !signals.is_empty() {
                                    self.process_pending_signals();
                                    last_status = 128 + *signals.last().unwrap();
                                    return last_status;
                                }
                            }
                            Err(nix::errno::Errno::EINTR) => {
                                // Interrupted — retry waitpid
                            }
                            _ => {
                                // Timeout or no self-pipe data — retry waitpid
                            }
                        }
                    }
                    Err(nix::errno::Errno::ECHILD) => {
                        eprintln!("kish: wait: pid {} is not a child of this shell", pid);
                        last_status = 127;
                        break;
                    }
                    Err(_) | Ok(_) => break,
                }
            }
        }

        last_status
    }

    fn builtin_jobs(&mut self, args: &[String]) -> i32 {
        let long_format = args.contains(&"-l".to_string());
        let pgid_only = args.contains(&"-p".to_string());

        // Collect job IDs first to avoid borrow issues
        let job_ids: Vec<crate::env::jobs::JobId> = self.env.process.jobs.all_jobs().map(|j| j.id).collect();

        for id in &job_ids {
            if pgid_only {
                if let Some(job) = self.env.process.jobs.get(*id) {
                    println!("{}", job.pgid.as_raw());
                }
            } else if long_format {
                if let Some(line) = self.env.process.jobs.format_job_long(*id) {
                    println!("{}", line);
                }
            } else if let Some(line) = self.env.process.jobs.format_job(*id) {
                println!("{}", line);
            }
        }

        // Mark done/terminated jobs as notified
        let pending = self.env.process.jobs.pending_notifications();
        for id in pending {
            self.env.process.jobs.mark_notified(id);
        }

        0
    }

    fn builtin_fg(&mut self, args: &[String]) -> i32 {
        use crate::env::jobs::{self, JobStatus};

        if !self.env.mode.options.monitor {
            eprintln!("kish: fg: no job control");
            return 1;
        }

        let job_id = if args.is_empty() {
            match self.env.process.jobs.current_id() {
                Some(id) => id,
                None => {
                    eprintln!("kish: fg: no current job");
                    return 1;
                }
            }
        } else {
            match self.env.process.jobs.resolve_job_spec(&args[0]) {
                Some(id) => id,
                None => {
                    eprintln!("kish: fg: {}: no such job", args[0]);
                    return 1;
                }
            }
        };

        let (pgid, command) = {
            let job = match self.env.process.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    eprintln!("kish: fg: job not found");
                    return 1;
                }
            };
            (job.pgid, job.command.clone())
        };

        // Print the command being foregrounded
        eprintln!("{}", command);

        // Update job state
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.foreground = true;
            if matches!(job.status, JobStatus::Stopped(_)) {
                job.status = JobStatus::Running;
            }
        }

        // Send SIGCONT to resume if stopped
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        // Give terminal to the job
        jobs::give_terminal(pgid).ok();

        // Wait for the job
        let status = self.wait_for_foreground_job(job_id);

        // Take terminal back
        jobs::take_terminal(self.env.process.shell_pgid).ok();

        status
    }

    fn builtin_bg(&mut self, args: &[String]) -> i32 {
        use crate::env::jobs::JobStatus;

        if !self.env.mode.options.monitor {
            eprintln!("kish: bg: no job control");
            return 1;
        }

        let job_id = if args.is_empty() {
            match self.env.process.jobs.current_id() {
                Some(id) => id,
                None => {
                    eprintln!("kish: bg: no current job");
                    return 1;
                }
            }
        } else {
            match self.env.process.jobs.resolve_job_spec(&args[0]) {
                Some(id) => id,
                None => {
                    eprintln!("kish: bg: {}: no such job", args[0]);
                    return 1;
                }
            }
        };

        let pgid = {
            let job = match self.env.process.jobs.get(job_id) {
                Some(j) => j,
                None => {
                    eprintln!("kish: bg: job not found");
                    return 1;
                }
            };
            if !matches!(job.status, JobStatus::Stopped(_)) {
                eprintln!("kish: bg: job {} not stopped", job_id);
                return 1;
            }
            job.pgid
        };

        // Update job state
        if let Some(job) = self.env.process.jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.foreground = false;
            eprintln!("[{}]+ {} &", job.id, job.command);
        }

        // Send SIGCONT
        nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGCONT).ok();

        0
    }

    /// Wait for a foreground job to complete or stop.
    fn wait_for_foreground_job(&mut self, job_id: crate::env::jobs::JobId) -> i32 {
        use crate::env::jobs::JobStatus;
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

        let pgid = match self.env.process.jobs.get(job_id) {
            Some(j) => j.pgid,
            None => return 1,
        };

        let mut last_status = 0;

        loop {
            match waitpid(nix::unistd::Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED)) {
                Ok(WaitStatus::Exited(pid, code)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Done(code));
                    last_status = code;
                    // Check if job is done
                    if self.all_job_processes_done(job_id) {
                        self.env.process.jobs.mark_notified(job_id);
                        self.env.process.jobs.remove_job(job_id);
                        break;
                    }
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    let code = 128 + sig as i32;
                    self.env.process.jobs.update_status(pid, JobStatus::Terminated(sig as i32));
                    last_status = code;
                    if self.all_job_processes_done(job_id) {
                        self.env.process.jobs.mark_notified(job_id);
                        self.env.process.jobs.remove_job(job_id);
                        break;
                    }
                }
                Ok(WaitStatus::Stopped(pid, sig)) => {
                    self.env.process.jobs.update_status(pid, JobStatus::Stopped(sig as i32));
                    if let Some(line) = self.env.process.jobs.format_job(job_id) {
                        eprintln!("{}", line);
                    }
                    last_status = 128 + sig as i32;
                    break;
                }
                Err(nix::errno::Errno::ECHILD) => {
                    self.env.process.jobs.remove_job(job_id);
                    break;
                }
                Err(nix::errno::Errno::EINTR) => {
                    self.process_pending_signals();
                    continue;
                }
                _ => break,
            }
        }

        last_status
    }

    /// Check if all processes in a job have finished (Done or Terminated).
    fn all_job_processes_done(&self, job_id: crate::env::jobs::JobId) -> bool {
        use crate::env::jobs::JobStatus;
        match self.env.process.jobs.get(job_id) {
            Some(job) => matches!(job.status, JobStatus::Done(_) | JobStatus::Terminated(_)),
            None => true,
        }
    }

    /// Display pending job notifications and clean up completed jobs.
    pub fn display_job_notifications(&mut self) {
        let pending = self.env.process.jobs.pending_notifications();
        for id in &pending {
            if let Some(line) = self.env.process.jobs.format_job(*id) {
                eprintln!("{}", line);
            }
            self.env.process.jobs.mark_notified(*id);
        }
        self.env.process.jobs.cleanup_notified();
    }
}

fn plugin_config_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config/kish/plugins.toml")
    } else {
        std::path::PathBuf::from("/nonexistent")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{
        AndOrList, AndOrOp, Command, CompleteCommand, Pipeline, Program, SeparatorOp,
        SimpleCommand, Word,
    };

    fn make_simple_cmd(words: &[&str]) -> SimpleCommand {
        SimpleCommand {
            assignments: vec![],
            words: words.iter().map(|s| Word::literal(s)).collect(),
            redirects: vec![],
        }
    }

    #[test]
    fn exec_builtin_true_returns_0() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["true"]);
        assert_eq!(exec.exec_simple_command(&cmd), 0);
        assert_eq!(exec.env.exec.last_exit_status, 0);
    }

    #[test]
    fn exec_builtin_false_returns_1() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["false"]);
        assert_eq!(exec.exec_simple_command(&cmd), 1);
        assert_eq!(exec.env.exec.last_exit_status, 1);
    }

    #[test]
    fn exec_external_true_returns_0() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["/usr/bin/true"]);
        assert_eq!(exec.exec_simple_command(&cmd), 0);
    }

    #[test]
    fn assignment_only_sets_var() {
        use crate::parser::ast::Assignment;
        let mut exec = Executor::new("kish", vec![]);
        let cmd = SimpleCommand {
            assignments: vec![Assignment {
                name: "MYVAR".to_string(),
                value: Some(Word::literal("hello")),
            }],
            words: vec![],
            redirects: vec![],
        };
        let status = exec.exec_simple_command(&cmd);
        assert_eq!(status, 0);
        assert_eq!(exec.env.vars.get("MYVAR"), Some("hello"));
    }

    #[test]
    fn exit_status_tracked() {
        let mut exec = Executor::new("kish", vec![]);
        // false sets last_exit_status to 1
        let false_cmd = make_simple_cmd(&["false"]);
        exec.exec_simple_command(&false_cmd);
        assert_eq!(exec.env.exec.last_exit_status, 1);

        // true resets it to 0
        let true_cmd = make_simple_cmd(&["true"]);
        exec.exec_simple_command(&true_cmd);
        assert_eq!(exec.env.exec.last_exit_status, 0);
    }

    #[test]
    fn test_single_command_pipeline() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let pipeline = Pipeline {
            negated: false,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal("true")],
                redirects: vec![],
            })],
        };
        assert_eq!(exec.exec_pipeline(&pipeline), 0);
    }

    #[test]
    fn test_negated_pipeline() {
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let pipeline = Pipeline {
            negated: true,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal("true")],
                redirects: vec![],
            })],
        };
        assert_eq!(exec.exec_pipeline(&pipeline), 1);
    }

    fn make_pipeline(word: &str) -> Pipeline {
        Pipeline {
            negated: false,
            commands: vec![Command::Simple(SimpleCommand {
                assignments: vec![],
                words: vec![Word::literal(word)],
                redirects: vec![],
            })],
        }
    }

    #[test]
    fn test_and_list_all_succeed() {
        // true && true → 0
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::And, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_and_list_first_fails() {
        // false && true → 1 (second not executed)
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![(AndOrOp::And, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 1);
    }

    #[test]
    fn test_or_list_first_fails() {
        // false || true → 0
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("false"),
            rest: vec![(AndOrOp::Or, make_pipeline("true"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_or_list_first_succeeds() {
        // true || false → 0 (second not executed)
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let and_or = AndOrList {
            first: make_pipeline("true"),
            rest: vec![(AndOrOp::Or, make_pipeline("false"))],
        };
        assert_eq!(exec.exec_and_or(&and_or), 0);
    }

    #[test]
    fn test_exec_program_sequential() {
        // true; false → 1 (last command status)
        let mut exec = Executor::new("kish".to_string(), vec![]);
        let program = Program {
            commands: vec![
                CompleteCommand {
                    items: vec![(
                        AndOrList {
                            first: make_pipeline("true"),
                            rest: vec![],
                        },
                        Some(SeparatorOp::Semi),
                    )],
                },
                CompleteCommand {
                    items: vec![(
                        AndOrList {
                            first: make_pipeline("false"),
                            rest: vec![],
                        },
                        None,
                    )],
                },
            ],
        };
        assert_eq!(exec.exec_program(&program), 1);
    }

    #[test]
    fn test_should_errexit_default_off() {
        let exec = Executor::new("kish", vec![]);
        assert!(!exec.should_errexit());
    }

    #[test]
    fn test_should_errexit_enabled() {
        let mut exec = Executor::new("kish", vec![]);
        exec.env.mode.options.errexit = true;
        assert!(exec.should_errexit());
    }

    #[test]
    fn test_with_errexit_suppressed() {
        let mut exec = Executor::new("kish", vec![]);
        exec.env.mode.options.errexit = true;
        assert!(exec.should_errexit());
        let result = exec.with_errexit_suppressed(|e| {
            assert!(!e.should_errexit());
            42
        });
        assert_eq!(result, 42);
        assert!(exec.should_errexit());
    }

    #[test]
    fn test_with_errexit_suppressed_nested() {
        let mut exec = Executor::new("kish", vec![]);
        exec.env.mode.options.errexit = true;
        exec.with_errexit_suppressed(|e| {
            assert!(!e.should_errexit());
            e.with_errexit_suppressed(|e2| {
                assert!(!e2.should_errexit());
            });
            assert!(!e.should_errexit());
        });
        assert!(exec.should_errexit());
    }
}
