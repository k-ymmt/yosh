pub mod aliases;
pub mod default_path;
pub mod exec_state;
pub mod jobs;
pub mod locale;
pub mod shell_mode;
pub mod traps;
pub mod vars;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use aliases::AliasStore;
pub use exec_state::{ExecState, FlowControl};
use jobs::JobTable;
use nix::unistd::{Pid, getpgrp, getpid, getppid};
pub use shell_mode::{InvocationOp, ShellMode, ShellOptions};
pub use traps::{TrapAction, TrapStore};
use vars::VarStore;

use crate::interactive::history::History;
use crate::parser::ast::FunctionDef;

/// Process and job management state.
#[derive(Debug, Clone)]
pub struct ProcessState {
    pub shell_pid: Pid,
    pub shell_pgid: Pid,
    pub jobs: JobTable,
}

/// The complete shell environment.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    pub vars: VarStore,
    pub exec: ExecState,
    pub process: ProcessState,
    pub mode: ShellMode,
    pub functions: HashMap<String, FunctionDef>,
    pub traps: TrapStore,
    pub aliases: AliasStore,
    pub history: History,
    pub shell_name: String,
    /// Cache of the POSIX default PATH (`confstr(_CS_PATH)`), computed
    /// lazily on first use. See `env::default_path::default_path()`.
    pub default_path_cache: OnceLock<String>,
    /// POSIX hash table: utility name → resolved absolute path.
    /// Auto-populated by `find_in_path` / `lookup_in_path` cache misses
    /// and by explicit `hash utility...` invocations. Cleared by
    /// `hash -r` and on `PATH` reassignment (POSIX §2.5.3).
    pub(crate) utility_hash: HashMap<String, HashEntry>,
}

/// One remembered `$PATH` lookup in the utility hash table.
#[derive(Debug, Clone)]
pub struct HashEntry {
    pub path: PathBuf,
    /// Number of times this cached entry satisfied a lookup without a
    /// fresh PATH walk. Shown by the `hash` builtin listing (format is
    /// implementation-defined per POSIX; bash also reports hit counts).
    pub hits: u64,
}

impl HashEntry {
    pub fn new(path: PathBuf) -> Self {
        HashEntry { path, hits: 0 }
    }
}

impl ShellEnv {
    /// Create a new ShellEnv, initializing variables from the process environment.
    ///
    /// `shell_name` is $0 (argv[0]), `args` are the positional parameters ($1, $2, ...).
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        // POSIX 2024 §2.5.3: the shell sets IFS to <space><tab><newline> at
        // startup, regardless of any value inherited from the environment.
        let _ = vars.set("IFS", " \t\n");
        // POSIX: "OPTIND shall be initialized to 1 when the shell is invoked."
        let _ = vars.set("OPTIND", "1");
        // POSIX §2.5.3: $PPID is the parent PID of the invoking shell,
        // captured once at startup. Subshells inherit the value.
        let _ = vars.set("PPID", getppid().as_raw().to_string());
        ShellEnv {
            vars,
            exec: ExecState {
                last_exit_status: 0,
                flow_control: None,
                trap_context_status: None,
                loop_depth: 0,
                indirection_level: 0,
                lineno: 0,
            },
            process: ProcessState {
                shell_pid: getpid(),
                shell_pgid: getpgrp(),
                jobs: JobTable::default(),
            },
            mode: ShellMode {
                options: ShellOptions::default(),
                is_interactive: false,
                flag_i: false,
                in_dot_script: false,
            },
            shell_name: shell_name.into(),
            functions: HashMap::new(),
            traps: TrapStore::default(),
            aliases: AliasStore::default(),
            history: History::new(),
            default_path_cache: OnceLock::new(),
            utility_hash: HashMap::new(),
        }
    }

    /// Set a shell variable. If `name == "PATH"`, the utility hash
    /// table is cleared after the successful assignment (POSIX
    /// §2.5.3). Returns `Err` only if the variable is readonly.
    pub fn assign_var(&mut self, name: &str, value: impl Into<String>) -> Result<(), String> {
        self.vars.set(name, value)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }

    /// Like [`Self::assign_var`] but honoring `set -a` (allexport),
    /// mirroring `VarStore::set_with_options`. Clears the utility hash
    /// on PATH writes (POSIX §2.5.3) — plain `PATH=/dir` statements and
    /// special-builtin prefix assignments must invalidate remembered
    /// utility locations just like `export PATH=...` does.
    pub fn assign_var_with_options(
        &mut self,
        name: &str,
        value: impl Into<String>,
        allexport: bool,
    ) -> Result<(), String> {
        self.vars.set_with_options(name, value, allexport)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }

    /// Unset a shell variable. If `name == "PATH"`, the utility hash
    /// table is cleared after the successful unset.
    pub fn unset_var(&mut self, name: &str) -> Result<(), String> {
        self.vars.unset(name)?;
        if name == "PATH" {
            self.utility_hash.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_env_construction() {
        let env = ShellEnv::new("yosh", vec!["arg1".to_string(), "arg2".to_string()]);
        assert_eq!(env.shell_name, "yosh");
        assert_eq!(env.vars.positional_params(), &["arg1", "arg2"]);
        assert_eq!(env.exec.last_exit_status, 0);
        // PID should be a positive number
        assert!(env.process.shell_pid.as_raw() > 0);
    }

    #[test]
    fn test_jobs_table() {
        let env = ShellEnv::new("yosh", vec![]);
        assert!(env.process.jobs.is_empty());
    }

    #[test]
    fn test_shell_pgid() {
        let env = ShellEnv::new("yosh", vec![]);
        assert!(env.process.shell_pgid.as_raw() > 0);
    }

    #[test]
    fn assign_var_clears_utility_hash_on_path_change() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.utility_hash.insert(
            "foo".to_string(),
            HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        env.assign_var("PATH", "/new").unwrap();
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn assign_var_leaves_utility_hash_for_non_path_var() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.utility_hash.insert(
            "foo".to_string(),
            HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        env.assign_var("OTHER", "x").unwrap();
        assert_eq!(env.utility_hash.len(), 1);
    }

    #[test]
    fn unset_var_clears_utility_hash_on_path_unset() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.assign_var("PATH", "/x").unwrap();
        env.utility_hash.insert(
            "foo".to_string(),
            HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        env.unset_var("PATH").unwrap();
        assert!(env.utility_hash.is_empty());
    }

    #[test]
    fn shell_env_new_seeds_optind_to_one() {
        let env = ShellEnv::new("yosh", vec![]);
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
    }

    #[test]
    fn nested_function_scopes_save_and_restore_optind_per_level() {
        // push → push → set OPTIND → inner pop restores the inner
        // caller's value → outer pop restores the outer caller's value.
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("OPTIND", "7").unwrap(); // outer caller's value

        env.vars.push_scope(vec![]); // saves "7", resets visible to "1"
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
        env.vars.set("OPTIND", "3").unwrap(); // inner caller's value

        env.vars.push_scope(vec![]); // saves "3", resets visible to "1"
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
        env.vars.set("OPTIND", "9").unwrap(); // innermost body's value
        assert_eq!(env.vars.get("OPTIND"), Some("9"));

        env.vars.pop_scope(); // must see the inner saved value
        assert_eq!(env.vars.get("OPTIND"), Some("3"));

        env.vars.pop_scope(); // must see the outer saved value
        assert_eq!(env.vars.get("OPTIND"), Some("7"));
    }

    #[test]
    fn readonly_optind_survives_scope_push_pop_round_trip() {
        // A readonly global OPTIND: push_scope still gives the function
        // body a fresh writable OPTIND=1 in the new scope, and pop_scope
        // skips the (failing) restore write, leaving the readonly global
        // value intact — no panic, no partial state.
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("OPTIND", "5").unwrap();
        env.vars.set_readonly("OPTIND");

        env.vars.push_scope(vec![]);
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
        // The scope-local OPTIND is a fresh variable, not the readonly one.
        env.vars.set("OPTIND", "2").unwrap();
        assert_eq!(env.vars.get("OPTIND"), Some("2"));

        env.vars.pop_scope();
        assert_eq!(env.vars.get("OPTIND"), Some("5"));
        assert!(env.vars.is_readonly("OPTIND"));
    }

    #[test]
    fn shell_env_new_overrides_inherited_optind_to_one() {
        // POSIX: "OPTIND shall be initialized to 1 when the shell is
        // invoked" — even when the parent process exported OPTIND=5.
        //
        // SAFETY: std::env::set_var/remove_var are serialized against
        // std::env::vars_os (used by VarStore::from_environ) by std's
        // internal environment lock; no test in this crate reads the
        // environment through raw libc while this runs.
        unsafe { std::env::set_var("OPTIND", "5") };
        let env = ShellEnv::new("yosh", vec![]);
        unsafe { std::env::remove_var("OPTIND") };
        assert_eq!(env.vars.get("OPTIND"), Some("1"));
        // The inherited variable came from the environment, so the
        // overwritten OPTIND must still be exported.
        let var = env.vars.get_var("OPTIND").expect("OPTIND must be set");
        assert!(var.exported, "inherited OPTIND stays exported after reset");
    }

    #[test]
    fn shell_env_new_seeds_ppid_to_getppid() {
        let env = ShellEnv::new("yosh", vec![]);
        let ppid = env.vars.get("PPID").expect("PPID must be set");
        let expected = nix::unistd::getppid().as_raw().to_string();
        assert_eq!(
            ppid, expected,
            "$PPID must equal nix::unistd::getppid() at shell start"
        );
        let n: i32 = ppid.parse().expect("PPID must parse as integer");
        assert!(n > 0, "$PPID must be a positive integer, got {}", n);
    }
}
