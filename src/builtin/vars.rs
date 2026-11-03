//! Variable-attribute special builtins: `export`, `readonly` and
//! `unset` (POSIX XCU §2.15).

use crate::builtin::options::{invalid_flag, scan_flag_words, strip_end_of_options};
use crate::builtin::output::write_stdout_decoded;
use crate::builtin::quote::quote_for_reinput;
use crate::env::ShellEnv;
use crate::env::vars::Variable;
use crate::error::ShellError;
use crate::parser::word::is_valid_name;

/// The attribute `export` / `readonly` give a variable.
#[derive(Clone, Copy)]
enum Attribute {
    Export,
    Readonly,
}

impl Attribute {
    /// The builtin's name, also the keyword of its re-input listing.
    fn keyword(self) -> &'static str {
        match self {
            Attribute::Export => "export",
            Attribute::Readonly => "readonly",
        }
    }

    fn is_set(self, var: &Variable) -> bool {
        match self {
            Attribute::Export => var.exported,
            Attribute::Readonly => var.readonly,
        }
    }

    fn apply(self, env: &mut ShellEnv, name: &str) {
        match self {
            Attribute::Export => env.vars.export(name),
            Attribute::Readonly => env.vars.set_readonly(name),
        }
    }

    /// `export name=value` / `readonly name=value` re-input line, or just
    /// `export name` for a name that has the attribute but no value.
    fn reinput_line(self, name: &str, value: Option<&str>) -> String {
        match value {
            Some(v) => format!("{} {}={}", self.keyword(), name, quote_for_reinput(v)),
            None => format!("{} {}", self.keyword(), name),
        }
    }
}

pub fn builtin_export(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    Ok(declare(Attribute::Export, args, env))
}

pub fn builtin_readonly(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    Ok(declare(Attribute::Readonly, args, env))
}

/// Shared body of `export` and `readonly`.
///
/// POSIX §2.15: with no operands or with `-p`, write the variables that
/// have the attribute in re-input format, including names without a
/// value (`export y`). Only `-p` in the first position triggers listing;
/// `-p` after operands or after `--` (end of options, XBD §12.2
/// Guideline 10) is validated as a bad identifier. Operands after `-p`
/// are NOT dropped (POSIX Issue 8 / bash behavior; dash ignores them):
///   - `export -p name=value` performs the assignment and sets the
///     attribute exactly like `export name=value` (matches bash).
///   - `export -p name` prints just that variable, if it has the
///     attribute (without setting it).
fn declare(attr: Attribute, args: &[String], env: &mut ShellEnv) -> i32 {
    let print_mode = args.first().map(String::as_str) == Some("-p");
    let operands = strip_end_of_options(&args[usize::from(print_mode)..]);

    if args.is_empty() || (print_mode && operands.is_empty()) {
        let mut listed: Vec<(String, Option<String>)> = env
            .vars
            .vars_iter()
            .filter(|(_, v)| attr.is_set(v))
            .map(|(k, v)| (k.to_string(), v.value.clone()))
            .collect();
        listed.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in listed {
            write_stdout_decoded(&attr.reinput_line(&name, value.as_deref()), true);
        }
        return 0;
    }

    let keyword = attr.keyword();
    let mut status = 0;
    for arg in operands {
        let name = match arg.find('=') {
            Some(pos) => &arg[..pos],
            None => arg.as_str(),
        };
        if !is_valid_name(name) {
            eprintln!("yosh: {}: `{}': not a valid identifier", keyword, name);
            status = 1;
            continue;
        }
        if let Some(pos) = arg.find('=') {
            let raw_value = &arg[pos + 1..];
            // assign_var (not vars.set): `readonly PATH=…` must
            // invalidate the utility hash like any other PATH write.
            if let Err(e) = env.assign_var(name, raw_value) {
                eprintln!("yosh: {}: {}", keyword, e);
                status = 1;
                continue;
            }
            attr.apply(env, name);
        } else if print_mode {
            // `export -p name`: print only this variable (if it has the
            // attribute).
            if let Some(v) = env.vars.get_var(name).filter(|v| attr.is_set(v)) {
                write_stdout_decoded(&attr.reinput_line(name, v.value()), true);
            }
        } else {
            attr.apply(env, name);
        }
    }
    status
}

pub fn builtin_unset(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    // POSIX §2.15 unset: unset [-fv] name...
    // -f removes function definitions; -v (default) removes variables.
    // Combining -f and -v is rejected with status 2.
    let mut mode_f = false;
    let mut mode_v = false;
    let flags = scan_flag_words(args, |ch| {
        match ch {
            'f' => mode_f = true,
            'v' => mode_v = true,
            _ => return false,
        }
        true
    });
    let idx = match flags {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("yosh: {}", invalid_flag("unset", e.flag));
            return Ok(2);
        }
    };
    if mode_f && mode_v {
        eprintln!("yosh: unset: cannot simultaneously unset a function and a variable");
        return Ok(2);
    }
    let unset_functions = mode_f;

    let mut status = 0;
    for name in &args[idx..] {
        if !is_valid_name(name) {
            eprintln!("yosh: unset: `{}': not a valid identifier", name);
            status = 1;
            continue;
        }
        if unset_functions {
            env.functions.remove(name);
        } else if let Err(e) = env.unset_var(name) {
            eprintln!("yosh: unset: {}", e);
            status = 1;
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use crate::builtin::special::exec_special_builtin;
    use crate::exec::Executor;

    #[test]
    fn unset_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("unset", &["1foo".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["1foo=v".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn export_rejects_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["1foo=v".to_string()], &mut executor);
        assert_eq!(status, 1);
    }

    #[test]
    fn unset_f_removes_function() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        assert!(executor.env.functions.contains_key("foo"));
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_f_keeps_variable_of_same_name() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        executor.env.vars.set("foo", "bar").unwrap();
        exec_special_builtin(
            "unset",
            &["-f".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(executor.env.vars.get("foo"), Some("bar"));
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_rejects_combined_f_v() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "-v".to_string(), "x".to_string()],
            &mut executor,
        );
        assert_eq!(status, 2);
    }

    #[test]
    fn unset_rejects_clustered_fv_flag() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-fv".to_string(), "x".to_string()],
            &mut executor,
        );
        assert_eq!(status, 2);
    }

    #[test]
    fn readonly_p_lists_readonly_var() {
        let mut executor = Executor::new("yosh", vec![]);
        exec_special_builtin("readonly", &["myvar=v".to_string()], &mut executor);
        let status = exec_special_builtin("readonly", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
        // The actual listing is on stdout (println!) which we don't capture here;
        // smoke-test via the e2e suite for output content.
    }

    #[test]
    fn export_p_with_assignment_assigns_and_exports() {
        // `export -p foo=v` must not silently drop the operand: the
        // assignment is performed and the variable exported (bash
        // behavior; POSIX Issue 8 permits operands with -p).
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "expvar=val1".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        let var = executor.env.vars.get_var("expvar").expect("must be set");
        assert_eq!(var.value(), Some("val1"));
        assert!(var.exported, "export -p name=value must export the var");
    }

    #[test]
    fn export_p_with_name_only_prints_without_exporting() {
        // `export -p name` prints that variable (stdout not captured
        // here) but must NOT export it — unlike plain `export name`.
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("plainvar", "v").unwrap();
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "plainvar".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        let var = executor.env.vars.get_var("plainvar").expect("must be set");
        assert!(
            !var.exported,
            "export -p name is print-only; it must not export"
        );
    }

    #[test]
    fn export_p_rejects_invalid_identifier_operand() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "1bad=v".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_p_with_assignment_assigns_and_marks_readonly() {
        // Same fix as export: `readonly -p foo=v` performs the
        // assignment and marks it readonly instead of dropping it.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "rovar=rv".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("rovar"), Some("rv"));
        assert!(executor.env.vars.is_readonly("rovar"));
    }

    #[test]
    fn readonly_p_with_name_only_prints_without_marking_readonly() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("notro", "v").unwrap();
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "notro".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(
            !executor.env.vars.is_readonly("notro"),
            "readonly -p name is print-only; it must not set the readonly flag"
        );
    }

    #[test]
    fn export_double_dash_then_assignment_succeeds() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["--".to_string(), "foo=hi".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), Some("hi"));
    }

    #[test]
    fn export_double_dash_alone_is_noop_rc0() {
        let mut executor = Executor::new("yosh", vec![]);
        let before = executor.env.vars.environ().len();
        let status = exec_special_builtin("export", &["--".to_string()], &mut executor);
        assert_eq!(status, 0);
        let after = executor.env.vars.environ().len();
        assert_eq!(before, after);
    }

    #[test]
    fn export_double_dash_then_dash_p_is_invalid_identifier() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn export_dash_p_alone_remains_listing() {
        // Regression guard: -p as the only arg still triggers listing rc=0.
        // The listing branch returns early before the helper is reached.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("export", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
    }

    #[test]
    fn export_p_then_double_dash_remains_listing() {
        // Regression guard: `export -p --` triggers listing because
        // `args[0] == "-p"` matches first; helper is never reached.
        // The `--` is harmless: the listing branch returns Ok(0) before
        // consume_end_of_options is called, so the operand is silently
        // ignored.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["-p".to_string(), "--".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
    }

    #[test]
    fn export_operand_then_dash_p_is_invalid_identifier() {
        // `export foo -p`: `-p` is not matched anywhere (export already
        // uses `args[0] == "-p"`), so foo is exported and `-p` is
        // rejected as a bad identifier (rc=1). Symmetric counterpart to
        // readonly_operand_then_dash_p_is_invalid_identifier.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "export",
            &["foo".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
        assert!(
            executor
                .env
                .vars
                .get_var("foo")
                .map(|v| v.exported)
                .unwrap_or(false)
        );
    }

    #[test]
    fn readonly_double_dash_then_assignment_succeeds() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "foo=ok".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), Some("ok"));
        assert!(
            executor
                .env
                .vars
                .get_var("foo")
                .map(|v| v.readonly)
                .unwrap_or(false)
        );
    }

    #[test]
    fn readonly_double_dash_alone_is_noop_rc0() {
        let mut executor = Executor::new("yosh", vec![]);
        let before = executor
            .env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .count();
        let status = exec_special_builtin("readonly", &["--".to_string()], &mut executor);
        assert_eq!(status, 0);
        let after = executor
            .env
            .vars
            .vars_iter()
            .filter(|(_, v)| v.readonly)
            .count();
        assert_eq!(before, after);
    }

    #[test]
    fn readonly_double_dash_then_dash_p_is_invalid_identifier() {
        // `--` ends options (XBD §12.2 Guideline 10), so the trailing
        // `-p` is validated as an operand and rejected as a bad
        // identifier — mirrors
        // export_double_dash_then_dash_p_is_invalid_identifier.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["--".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn readonly_p_then_double_dash_remains_listing() {
        // Regression guard: `readonly -p --` triggers listing because
        // `args[0] == "-p"` matches first; helper is never reached.
        // The `--` is harmless: the listing branch returns Ok(0) before
        // consume_end_of_options is called, so the operand is silently
        // ignored. Mirrors export_p_then_double_dash_remains_listing.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["-p".to_string(), "--".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
    }

    #[test]
    fn readonly_operand_then_dash_p_is_invalid_identifier() {
        // `readonly foo -p`: `-p` is no longer matched anywhere in args,
        // so foo is set readonly and `-p` is rejected as a bad
        // identifier (rc=1). Symmetric with export's operand-then-option
        // handling.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "readonly",
            &["foo".to_string(), "-p".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
        assert!(
            executor
                .env
                .vars
                .get_var("foo")
                .map(|v| v.readonly)
                .unwrap_or(false)
        );
    }

    #[test]
    fn readonly_dash_p_alone_remains_listing() {
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin("readonly", &["-p".to_string()], &mut executor);
        assert_eq!(status, 0);
    }

    #[test]
    fn unset_double_dash_unsets_following_name() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.env.vars.set("foo", "v").unwrap();
        let status = exec_special_builtin(
            "unset",
            &["--".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert_eq!(executor.env.vars.get("foo"), None);
    }

    #[test]
    fn unset_f_then_double_dash_unsets_function() {
        let mut executor = Executor::new("yosh", vec![]);
        executor.eval_string("foo() { :; }");
        let status = exec_special_builtin(
            "unset",
            &["-f".to_string(), "--".to_string(), "foo".to_string()],
            &mut executor,
        );
        assert_eq!(status, 0);
        assert!(!executor.env.functions.contains_key("foo"));
    }

    #[test]
    fn unset_v_then_double_dash_invalid_operand() {
        // After `-v --`, `-f` is an operand and must fail identifier validation.
        let mut executor = Executor::new("yosh", vec![]);
        let status = exec_special_builtin(
            "unset",
            &["-v".to_string(), "--".to_string(), "-f".to_string()],
            &mut executor,
        );
        assert_eq!(status, 1);
    }
}
