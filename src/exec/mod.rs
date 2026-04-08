pub mod command;
pub mod pipeline;
pub mod redirect;

use std::ffi::CString;

use nix::unistd::{execvp, fork, ForkResult};

use crate::builtin::{classify_builtin, exec_regular_builtin, BuiltinKind};
use crate::builtin::special::exec_special_builtin;
use crate::env::{FlowControl, ShellEnv};
use crate::expand::expand_words;
use crate::parser::ast::{
    AndOrList, AndOrOp, Assignment, CaseItem, CaseTerminator, Command, CompoundCommand,
    CompoundCommandKind, CompleteCommand, FunctionDef, Program, Redirect, SeparatorOp,
    SimpleCommand, Word,
};

use command::wait_child;
use redirect::RedirectState;

pub struct Executor {
    pub env: ShellEnv,
}

impl Executor {
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        Executor {
            env: ShellEnv::new(shell_name, args),
        }
    }

    /// Dispatch a `Command` to the appropriate execution path.
    pub fn exec_command(&mut self, cmd: &Command) -> i32 {
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

    /// Execute a compound command, applying any redirects around it.
    fn exec_compound_command(
        &mut self,
        compound: &CompoundCommand,
        redirects: &[Redirect],
    ) -> i32 {
        let mut redirect_state = RedirectState::new();
        if let Err(e) = redirect_state.apply(redirects, &mut self.env, true) {
            eprintln!("kish: {}", e);
            self.env.last_exit_status = 1;
            return 1;
        }

        let status = match &compound.kind {
            CompoundCommandKind::BraceGroup { body } => self.exec_brace_group(body),
            CompoundCommandKind::Subshell { body } => self.exec_subshell(body),
            CompoundCommandKind::If {
                condition,
                then_part,
                elif_parts,
                else_part,
            } => self.exec_if(condition, then_part, elif_parts, else_part),
            CompoundCommandKind::While { condition, body } => {
                self.exec_loop(condition, body, false)
            }
            CompoundCommandKind::Until { condition, body } => {
                self.exec_loop(condition, body, true)
            }
            CompoundCommandKind::For { var, words, body } => {
                self.exec_for(var, words, body)
            }
            CompoundCommandKind::Case { word, items } => self.exec_case(word, items),
        };

        redirect_state.restore();
        self.env.last_exit_status = status;
        status
    }

    /// Execute a list of complete commands (a compound-list / body).
    /// Checks for flow control signals after each command.
    fn exec_body(&mut self, body: &[CompleteCommand]) -> i32 {
        let mut status = 0;
        for cmd in body {
            status = self.exec_complete_command(cmd);
            if self.env.flow_control.is_some() {
                break;
            }
        }
        status
    }

    fn exec_brace_group(&mut self, body: &[CompleteCommand]) -> i32 {
        self.exec_body(body)
    }
    fn exec_subshell(&mut self, body: &[CompleteCommand]) -> i32 {
        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                let status = self.exec_body(body);
                std::process::exit(status);
            }
            Ok(ForkResult::Parent { child }) => command::wait_child(child),
        }
    }
    fn exec_if(
        &mut self,
        condition: &[CompleteCommand],
        then_part: &[CompleteCommand],
        elif_parts: &[(Vec<CompleteCommand>, Vec<CompleteCommand>)],
        else_part: &Option<Vec<CompleteCommand>>,
    ) -> i32 {
        let cond_status = self.exec_body(condition);
        if self.env.flow_control.is_some() {
            return cond_status;
        }

        if cond_status == 0 {
            return self.exec_body(then_part);
        }

        for (elif_cond, elif_body) in elif_parts {
            let cond_status = self.exec_body(elif_cond);
            if self.env.flow_control.is_some() {
                return cond_status;
            }
            if cond_status == 0 {
                return self.exec_body(elif_body);
            }
        }

        if let Some(else_body) = else_part {
            return self.exec_body(else_body);
        }

        0
    }
    /// Execute a while or until loop.
    /// `until=false` → while (run while condition succeeds)
    /// `until=true`  → until (run while condition fails)
    fn exec_loop(
        &mut self,
        condition: &[CompleteCommand],
        body: &[CompleteCommand],
        until: bool,
    ) -> i32 {
        let mut status = 0;
        loop {
            let cond_status = self.exec_body(condition);
            if self.env.flow_control.is_some() {
                return cond_status;
            }
            let should_run = if until {
                cond_status != 0
            } else {
                cond_status == 0
            };
            if !should_run {
                break;
            }

            status = self.exec_body(body);

            match self.env.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                    // n <= 1: continue this loop (re-evaluate condition)
                }
                Some(other) => {
                    self.env.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        status
    }
    fn exec_for(
        &mut self,
        var: &str,
        words: &Option<Vec<Word>>,
        body: &[CompleteCommand],
    ) -> i32 {
        let items: Vec<String> = match words {
            Some(word_list) => expand_words(&mut self.env, word_list),
            None => self.env.positional_params.clone(),
        };

        let mut status = 0;
        for item in &items {
            if let Err(e) = self.env.vars.set(var, item.as_str()) {
                eprintln!("kish: {}", e);
                return 1;
            }

            status = self.exec_body(body);

            match self.env.flow_control.take() {
                Some(FlowControl::Break(n)) => {
                    if n > 1 {
                        self.env.flow_control = Some(FlowControl::Break(n - 1));
                    }
                    break;
                }
                Some(FlowControl::Continue(n)) => {
                    if n > 1 {
                        self.env.flow_control = Some(FlowControl::Continue(n - 1));
                        break;
                    }
                    // n <= 1: continue this loop
                }
                Some(other) => {
                    self.env.flow_control = Some(other);
                    break;
                }
                None => {}
            }
        }
        status
    }
    fn exec_case(&mut self, word: &Word, items: &[CaseItem]) -> i32 {
        let case_word = crate::expand::expand_word_to_string(&mut self.env, word);
        let mut status = 0;
        let mut falling_through = false;

        for item in items {
            if !falling_through {
                let mut matched = false;
                for pattern in &item.patterns {
                    let pat = crate::expand::expand_word_to_string(&mut self.env, pattern);
                    if crate::expand::pattern::matches(&pat, &case_word) {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    continue;
                }
            }

            status = self.exec_body(&item.body);
            if self.env.flow_control.is_some() {
                break;
            }

            match item.terminator {
                CaseTerminator::Break => break,
                CaseTerminator::FallThrough => {
                    falling_through = true;
                }
            }
        }

        status
    }

    /// Invoke a function: save/restore positional params, execute body.
    fn exec_function_call(&mut self, func_def: &FunctionDef, args: &[String]) -> i32 {
        let saved_params =
            std::mem::replace(&mut self.env.positional_params, args.to_vec());

        let status =
            self.exec_compound_command(&func_def.body, &func_def.redirects);

        // Handle return flow control
        let final_status = match self.env.flow_control.take() {
            Some(FlowControl::Return(s)) => s,
            Some(other) => {
                self.env.flow_control = Some(other);
                status
            }
            None => status,
        };

        self.env.positional_params = saved_params;
        self.env.last_exit_status = final_status;
        final_status
    }

    /// Execute a simple command (assignments, builtins, or external programs).
    pub fn exec_simple_command(&mut self, cmd: &SimpleCommand) -> i32 {
        // Expand all words
        let expanded = expand_words(&mut self.env, &cmd.words);

        // Assignment-only command (no words)
        if expanded.is_empty() {
            // Track the exit status from any command substitutions in the assignments.
            // POSIX: the exit status of an assignment-only command is the exit status
            // of the last command substitution performed during expansion.
            let mut last_cmd_sub_status = 0i32;
            for assignment in &cmd.assignments {
                // Reset before each expansion so we can capture per-assignment status
                self.env.last_exit_status = 0;
                let value = assignment
                    .value
                    .as_ref()
                    .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
                    .unwrap_or_default();
                // Capture the status set by any command substitution during expansion
                last_cmd_sub_status = self.env.last_exit_status;
                if let Err(e) = self.env.vars.set(&assignment.name, value) {
                    eprintln!("kish: {}", e);
                    self.env.last_exit_status = 1;
                    return 1;
                }
            }
            self.env.last_exit_status = last_cmd_sub_status;
            return last_cmd_sub_status;
        }

        let command_name = expanded[0].clone();
        let args: Vec<String> = expanded[1..].to_vec();

        // Check for function call (before builtins, matching POSIX lookup order)
        if let Some(func_def) = self.env.functions.get(&command_name).cloned() {
            let mut redirect_state = RedirectState::new();
            if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                eprintln!("kish: {}", e);
                self.env.last_exit_status = 1;
                return 1;
            }
            let status = self.exec_function_call(&func_def, &args);
            redirect_state.restore();
            self.env.last_exit_status = status;
            return status;
        }

        match classify_builtin(&command_name) {
            BuiltinKind::Special => {
                // Special builtins: prefix assignments persist in current env
                for assignment in &cmd.assignments {
                    let value = assignment
                        .value
                        .as_ref()
                        .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
                        .unwrap_or_default();
                    if let Err(e) = self.env.vars.set(&assignment.name, value) {
                        eprintln!("kish: {}", e);
                        self.env.last_exit_status = 1;
                        return 1;
                    }
                }
                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.env.last_exit_status = 1;
                    return 1;
                }
                let status = exec_special_builtin(&command_name, &args, self);
                redirect_state.restore();
                self.env.last_exit_status = status;
                status
            }
            BuiltinKind::Regular => {
                // Regular builtins: prefix assignments are temporary
                let saved = self.apply_temp_assignments(&cmd.assignments);
                let mut redirect_state = RedirectState::new();
                if let Err(e) = redirect_state.apply(&cmd.redirects, &mut self.env, true) {
                    eprintln!("kish: {}", e);
                    self.restore_assignments(saved);
                    self.env.last_exit_status = 1;
                    return 1;
                }
                let status = exec_regular_builtin(&command_name, &args, &mut self.env);
                redirect_state.restore();
                self.restore_assignments(saved);
                self.env.last_exit_status = status;
                status
            }
            BuiltinKind::NotBuiltin => {
                let env_vars = self.build_env_vars(&cmd.assignments);
                let status = self.exec_external_with_redirects(
                    &command_name, &args, &env_vars, &cmd.redirects,
                );
                self.env.last_exit_status = status;
                status
            }
        }
    }

    /// Merge exported shell variables with command-specific assignments.
    pub fn build_env_vars(&mut self, assignments: &[Assignment]) -> Vec<(String, String)> {
        let mut vars = self.env.vars.to_environ();
        for assign in assignments {
            let value = assign
                .value
                .as_ref()
                .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
                .unwrap_or_default();
            // Replace existing or push new
            if let Some(entry) = vars.iter_mut().find(|(k, _)| k == &assign.name) {
                entry.1 = value;
            } else {
                vars.push((assign.name.clone(), value));
            }
        }
        vars
    }

    /// Fork, apply redirects in child, exec the command, wait in parent.
    pub fn exec_external_with_redirects(
        &mut self,
        cmd: &str,
        args: &[String],
        env_vars: &[(String, String)],
        redirects: &[crate::parser::ast::Redirect],
    ) -> i32 {
        // Build argv CStrings
        let c_cmd = match CString::new(cmd) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("kish: {}: invalid command name", cmd);
                return 127;
            }
        };
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
        c_args.push(c_cmd.clone());
        for a in args {
            match CString::new(a.as_str()) {
                Ok(s) => c_args.push(s),
                Err(_) => {
                    eprintln!("kish: {}: invalid argument", a);
                    return 1;
                }
            }
        }

        match unsafe { fork() } {
            Err(e) => {
                eprintln!("kish: fork: {}", e);
                1
            }
            Ok(ForkResult::Child) => {
                // Apply redirects (no need to save, we're in the child)
                let mut redir_state = RedirectState::new();
                if let Err(e) = redir_state.apply(redirects, &mut self.env, false) {
                    eprintln!("kish: {}", e);
                    std::process::exit(1);
                }

                // Set environment variables
                for (k, v) in env_vars {
                    // SAFETY: single-threaded child after fork
                    unsafe { std::env::set_var(k, v) };
                }

                let err = execvp(&c_cmd, &c_args).unwrap_err();
                use nix::errno::Errno;
                let exit_code = match err {
                    Errno::ENOENT => {
                        eprintln!("kish: {}: command not found", cmd);
                        127
                    }
                    Errno::EACCES => {
                        eprintln!("kish: {}: permission denied", cmd);
                        126
                    }
                    _ => {
                        eprintln!("kish: {}: {}", cmd, err);
                        127
                    }
                };
                std::process::exit(exit_code);
            }
            Ok(ForkResult::Parent { child }) => wait_child(child),
        }
    }

    /// Execute an AND-OR list.
    pub fn exec_and_or(&mut self, and_or: &AndOrList) -> i32 {
        let mut status = self.exec_pipeline(&and_or.first);
        if self.env.flow_control.is_some() {
            return status;
        }

        for (op, pipeline) in &and_or.rest {
            match op {
                AndOrOp::And => {
                    if status == 0 {
                        status = self.exec_pipeline(pipeline);
                    }
                }
                AndOrOp::Or => {
                    if status != 0 {
                        status = self.exec_pipeline(pipeline);
                    }
                }
            }
            if self.env.flow_control.is_some() {
                break;
            }
        }

        self.env.last_exit_status = status;
        status
    }

    /// Reap any zombie background children without blocking.
    fn reap_zombies(&self) {
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG),
            ) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_) => continue, // Reaped a zombie, check for more
                Err(_) => break,   // No children or error
            }
        }
    }

    /// Execute a complete command (list of AND-OR lists with separators).
    pub fn exec_complete_command(&mut self, cmd: &CompleteCommand) -> i32 {
        // Reap any finished background children before forking new ones
        self.reap_zombies();

        let mut status = 0;

        for (and_or, separator) in &cmd.items {
            if separator == &Some(SeparatorOp::Amp) {
                // Background: fork child to execute, parent continues with status 0
                match unsafe { fork() } {
                    Err(e) => {
                        eprintln!("kish: fork: {}", e);
                        status = 1;
                    }
                    Ok(ForkResult::Child) => {
                        let s = self.exec_and_or(and_or);
                        std::process::exit(s);
                    }
                    Ok(ForkResult::Parent { child }) => {
                        // Track last background PID ($!)
                        self.env.last_bg_pid = Some(child.as_raw());
                        // Parent continues; background job status = 0
                        status = 0;
                    }
                }
            } else {
                // Sequential execution
                status = self.exec_and_or(and_or);
            }
            if self.env.flow_control.is_some() {
                break;
            }
        }

        self.env.last_exit_status = status;
        status
    }

    /// Execute a program (sequence of complete commands).
    pub fn exec_program(&mut self, program: &Program) -> i32 {
        let mut status = 0;
        for cmd in &program.commands {
            status = self.exec_complete_command(cmd);
        }
        self.env.last_exit_status = status;
        status
    }

    /// Apply prefix assignments temporarily, returning saved values for later restoration.
    fn apply_temp_assignments(
        &mut self,
        assignments: &[crate::parser::ast::Assignment],
    ) -> Vec<(String, Option<String>)> {
        let mut saved = Vec::new();
        for assignment in assignments {
            let old_val = self.env.vars.get(&assignment.name).map(|s| s.to_string());
            saved.push((assignment.name.clone(), old_val));
            let value = assignment
                .value
                .as_ref()
                .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
                .unwrap_or_default();
            let _ = self.env.vars.set(&assignment.name, value);
        }
        saved
    }

    /// Restore variables to their pre-assignment values.
    fn restore_assignments(&mut self, saved: Vec<(String, Option<String>)>) {
        for (name, old_val) in saved {
            match old_val {
                Some(val) => {
                    let _ = self.env.vars.set(&name, val);
                }
                None => {
                    let _ = self.env.vars.unset(&name);
                }
            }
        }
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
        assert_eq!(exec.env.last_exit_status, 0);
    }

    #[test]
    fn exec_builtin_false_returns_1() {
        let mut exec = Executor::new("kish", vec![]);
        let cmd = make_simple_cmd(&["false"]);
        assert_eq!(exec.exec_simple_command(&cmd), 1);
        assert_eq!(exec.env.last_exit_status, 1);
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
        assert_eq!(exec.env.last_exit_status, 1);

        // true resets it to 0
        let true_cmd = make_simple_cmd(&["true"]);
        exec.exec_simple_command(&true_cmd);
        assert_eq!(exec.env.last_exit_status, 0);
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
}
