//! `set -x` (xtrace) output for simple commands.

use crate::env::ShellEnv;

use super::super::Executor;

/// Build the `set -x` trace prefix from PS4.
///
/// PS4 is expanded (parameter/command/arithmetic) like a double-quoted
/// string, then its first character appears `indirection_level + 1` times
/// total (the original plus one per nesting level) to indicate
/// function/dot-script nesting (POSIX XCU 2.5.3).
/// `$?` is preserved across any command substitution inside PS4, and an
/// expansion error in PS4 is diagnosed without aborting (both handled by
/// `expand_dquoted`).
fn xtrace_prefix(env: &mut ShellEnv) -> String {
    let raw = env.vars.get("PS4").unwrap_or("+ ").to_string();

    // Expand PS4 with xtrace off: a command substitution in PS4
    // (`PS4='$(echo X)> '`) runs in a forked child that inherits the
    // options, and with xtrace still on its own commands would expand
    // PS4 again — unbounded recursion. bash traces nothing inside PS4.
    let saved_xtrace = std::mem::replace(&mut env.mode.options.xtrace, false);
    let expanded = crate::expand::expand_dquoted(env, &raw);
    env.mode.options.xtrace = saved_xtrace;

    let level = env.exec.indirection_level;
    if level == 0 || expanded.is_empty() {
        return expanded;
    }

    // Replicate only the first character `level + 1` times total.
    let mut chars = expanded.chars();
    let first = chars.next().expect("non-empty checked above");
    let rest = chars.as_str();
    let mut out = String::with_capacity(expanded.len() + level * first.len_utf8());
    for _ in 0..=level {
        out.push(first);
    }
    out.push_str(rest);
    out
}

impl Executor {
    /// `set -x` trace line for a simple command with a command name:
    /// prefix assignments first, then the command words, on one line
    /// (`+ x=1 echo hi`, matching dash). Emitted after the assignment
    /// values are expanded so the trace shows their final values.
    pub(super) fn xtrace_simple(
        &mut self,
        assignments: &[(String, String)],
        name: &str,
        args: &[String],
    ) {
        if !self.env.mode.options.xtrace {
            return;
        }
        let mut line = xtrace_prefix(&mut self.env);
        for (n, v) in assignments {
            line.push_str(n);
            line.push('=');
            line.push_str(v);
            line.push(' ');
        }
        line.push_str(name);
        for a in args {
            line.push(' ');
            line.push_str(a);
        }
        crate::byteenc::write_stderr_decoded_line(&line);
    }

    /// `set -x` trace line for one assignment of an assignment-only
    /// command (`+ x=1`).
    pub(super) fn xtrace_assignment(&mut self, name: &str, value: &str) {
        if !self.env.mode.options.xtrace {
            return;
        }
        let prefix = xtrace_prefix(&mut self.env);
        crate::byteenc::write_stderr_decoded_line(&format!("{}{}={}", prefix, name, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xtrace_prefix_uses_ps4_when_set() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "TRACE> ").unwrap();
        assert_eq!(xtrace_prefix(&mut env), "TRACE> ");
    }

    #[test]
    fn test_xtrace_prefix_default_when_ps4_unset() {
        let mut env = ShellEnv::new("yosh", vec![]);
        // PS4 is not set by ShellEnv::new, so the helper falls back to "+ ".
        assert_eq!(xtrace_prefix(&mut env), "+ ");
    }

    #[test]
    fn test_xtrace_prefix_expands_parameter() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.exec.lineno = 42;
        env.vars.set("PS4", "L$LINENO> ").unwrap();
        assert_eq!(xtrace_prefix(&mut env), "L42> ");
    }

    #[test]
    fn test_xtrace_prefix_expands_last_exit_status() {
        // `$?` inside PS4 expands to the saved status (then is restored).
        let mut env = ShellEnv::new("yosh", vec![]);
        env.exec.last_exit_status = 7;
        env.vars.set("PS4", "$?> ").unwrap();
        assert_eq!(xtrace_prefix(&mut env), "7> ");
        assert_eq!(env.exec.last_exit_status, 7);
    }

    #[test]
    fn test_xtrace_prefix_replicates_first_char_by_level() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "+ ").unwrap();
        env.exec.indirection_level = 0;
        assert_eq!(xtrace_prefix(&mut env), "+ ");
        env.exec.indirection_level = 1;
        assert_eq!(xtrace_prefix(&mut env), "++ ");
        env.exec.indirection_level = 2;
        assert_eq!(xtrace_prefix(&mut env), "+++ ");
    }

    #[test]
    fn test_xtrace_prefix_replicates_only_first_char_multichar() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "TRACE> ").unwrap();
        env.exec.indirection_level = 1;
        assert_eq!(xtrace_prefix(&mut env), "TTRACE> ");
    }

    #[test]
    fn test_xtrace_prefix_empty_ps4_stays_empty() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("PS4", "").unwrap();
        env.exec.indirection_level = 3;
        assert_eq!(xtrace_prefix(&mut env), "");
    }

    #[test]
    fn test_xtrace_prefix_preserves_exit_status() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.exec.last_exit_status = 7;
        env.vars.set("PS4", "$(exit 3)> ").unwrap();
        let _ = xtrace_prefix(&mut env);
        assert_eq!(env.exec.last_exit_status, 7);
    }
}
