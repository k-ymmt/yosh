use super::super::Executor;
use super::executor_hosted_builtin;

impl Executor {
    /// POSIX `command` builtin. Dispatches by verbosity:
    /// - Brief (`-v`) / Verbose (`-V`) → print and return exit status
    /// - Execute (`-p` or no flag) → handled in later tasks (returns 1 for now)
    pub(crate) fn builtin_command(&mut self, args: &[String]) -> i32 {
        use crate::builtin::command::{Verbosity, parse_flags, render_brief, render_verbose};

        let parsed = match parse_flags(args) {
            Ok(p) => p,
            Err(msg) => {
                eprintln!("yosh: {}", msg);
                return 2;
            }
        };

        match parsed.verbose {
            Verbosity::Brief => {
                let (out, code) = render_brief(&mut self.env, &parsed.name);
                if !out.is_empty() {
                    println!("{}", out);
                }
                code
            }
            Verbosity::Verbose => {
                let (out, err, code) = render_verbose(&mut self.env, &parsed.name);
                if !out.is_empty() {
                    println!("{}", out);
                }
                if !err.is_empty() {
                    eprintln!("{}", err);
                }
                code
            }
            Verbosity::Execute => {
                if parsed.use_default_path {
                    self.exec_command_with_default_path(&parsed.name, &parsed.rest)
                } else {
                    // No-flag path (function-skip): implemented in the next task.
                    self.exec_command_skip_functions(&parsed.name, &parsed.rest)
                }
            }
        }
    }

    /// Run one of the regular builtins whose implementation lives on the
    /// Executor (see [`executor_hosted_builtin`]). Returns `None` when
    /// `name` is not one of them. Used by the `command` execution paths,
    /// which must honor these as builtins (POSIX: `command` bypasses
    /// functions only).
    fn exec_executor_hosted_builtin(&mut self, name: &str, args: &[String]) -> Option<i32> {
        executor_hosted_builtin(name).map(|builtin| builtin(self, args))
    }

    /// `command -p name args...`: look up `name` via the POSIX default PATH
    /// (ignoring $PATH entirely) and exec it. Builtins are still honored
    /// for the name: POSIX says `command -p` runs the named utility in
    /// preference over functions, but builtins are part of the utility set.
    pub(crate) fn exec_command_with_default_path(&mut self, name: &str, args: &[String]) -> i32 {
        use crate::builtin::special::exec_special_builtin;
        use crate::builtin::{BuiltinKind, classify_builtin, exec_regular_builtin};
        use crate::env::default_path::default_path;

        // If `name` is a builtin, run the builtin (POSIX: command -p still
        // runs builtins; -p only affects external lookup).
        match classify_builtin(name) {
            BuiltinKind::Special => {
                let status = exec_special_builtin(name, args, self);
                return status;
            }
            BuiltinKind::Regular => {
                // POSIX: `command` bypasses FUNCTIONS only — built-ins are
                // still executed, including the Executor-hosted ones
                // (`command wait` waits; running a PATH binary instead
                // returned immediately).
                if let Some(status) = self.exec_executor_hosted_builtin(name, args) {
                    return status;
                }
                return exec_regular_builtin(name, args, &mut self.env);
            }
            BuiltinKind::NotBuiltin => {}
        }

        use crate::exec::command::{PathLookup, lookup_in_path_uncached};

        // Uncached lookup: `utility_hash` is keyed by name alone and is
        // (conceptually) scoped to $PATH. A `command -p` hit found via the
        // POSIX *default* PATH must not seed the cache, or a later plain
        // `name` lookup under a different $PATH would reuse it — the same
        // cache-key mismatch fixed for `PATH=dir cmd` prefix overrides in
        // edb5254, which also resolves uncached.
        let dp = default_path(&self.env).to_string();
        match lookup_in_path_uncached(name, &dp) {
            // Simple-command redirects were already applied by the
            // `command` special-case handler in exec_simple_command, and
            // prefix assignments by apply_temp_assignments — hence the
            // empty redirect/override slices.
            PathLookup::Executable(p) => self.spawn_external_at_path(&p, name, args, &[], &[]),
            PathLookup::NotExecutable(p) => {
                eprintln!("yosh: command: {}: permission denied", p.display());
                126
            }
            PathLookup::NotFound => {
                eprintln!("yosh: command: {}: not found", name);
                127
            }
        }
    }

    /// `command name args...`: execute `name` using the current $PATH but
    /// bypassing shell functions. Aliases are already handled (they're
    /// expanded at parse time, so `command` arrived here only if the
    /// parser saw `command` itself, not the expanded alias).
    pub(crate) fn exec_command_skip_functions(&mut self, name: &str, args: &[String]) -> i32 {
        use crate::builtin::special::exec_special_builtin;
        use crate::builtin::{BuiltinKind, classify_builtin, exec_regular_builtin};

        // Builtins take precedence over external; functions are deliberately
        // skipped.
        match classify_builtin(name) {
            BuiltinKind::Special => return exec_special_builtin(name, args, self),
            BuiltinKind::Regular => {
                // POSIX: `command name` bypasses functions only — the
                // Executor-hosted builtins (wait/fg/bg/jobs/command) must
                // run as builtins here too, not be looked up in PATH.
                if let Some(status) = self.exec_executor_hosted_builtin(name, args) {
                    return status;
                }
                return exec_regular_builtin(name, args, &mut self.env);
            }
            BuiltinKind::NotBuiltin => {}
        }

        // External: resolve via $PATH (not the POSIX default path).
        use crate::exec::command::{PathLookup, lookup_in_path};

        let path_var = self
            .env
            .vars
            .get("PATH")
            .map(|s| s.to_string())
            .unwrap_or_default();
        match lookup_in_path(name, &path_var, &mut self.env.utility_hash) {
            // See exec_command_with_default_path for why the redirect and
            // override slices are empty here.
            PathLookup::Executable(p) => self.spawn_external_at_path(&p, name, args, &[], &[]),
            PathLookup::NotExecutable(p) => {
                eprintln!("yosh: command: {}: permission denied", p.display());
                126
            }
            PathLookup::NotFound => {
                eprintln!("yosh: command: {}: not found", name);
                127
            }
        }
    }
}

#[cfg(test)]
mod tests {

    // ── command -p must not seed the $PATH-scoped utility hash (SP2) ──

    #[test]
    fn command_p_lookup_does_not_seed_utility_hash() {
        let mut exec = crate::exec::Executor::new("yosh", vec![]);
        // `sh` is not a builtin, so this exercises the default-PATH
        // external lookup path.
        let status =
            exec.exec_command_with_default_path("sh", &["-c".to_string(), ":".to_string()]);
        assert_eq!(status, 0, "command -p sh -c : must succeed");
        assert!(
            !exec.env.utility_hash.contains_key("sh"),
            "a default-PATH hit must not be reusable by later $PATH lookups"
        );
    }
}
