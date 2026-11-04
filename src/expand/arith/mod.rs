//! Arithmetic expansion `$((expr))` (POSIX §2.6.4).
//!
//! [`evaluate`] expands the text's `$` references, then evaluates it with
//! the one-pass evaluator ([`one_pass`]) or, for source text seen before,
//! from a cached tree ([`cache`]) that mirrors it. Both share the
//! tokenizer ([`token`]), the operator semantics ([`ops`]) and the
//! variable-operand helpers ([`var`]).

use crate::env::ShellEnv;
use crate::error::{ExpansionErrorKind, ShellError};

mod cache;
mod one_pass;
mod ops;
mod token;
mod var;

use one_pass::ArithParser;

/// Maximum depth of recursive variable-as-expression evaluation
/// (`x=y; y=1; $((x))`). Prevents infinite loops like `x=x`.
const MAX_RECURSION_DEPTH: u32 = 64;

/// Maximum syntactic nesting depth (parentheses, unary operators,
/// ternary/assignment chains) of one arithmetic evaluation, counted
/// across recursive variable-as-expression evaluation. The evaluator is
/// recursive descent, so unbounded nesting (`$(( ((((…1…)))) ))`) would
/// overflow the native stack and abort the shell; past this depth it
/// reports an ordinary arithmetic error instead. One parenthesis level
/// costs ~13 KiB of stack in a debug build (~17 descent frames), which
/// overflowed the 8 MiB main stack near 620 levels; 256 keeps the worst
/// case near 3 MiB there and far less in release builds, while staying
/// well beyond any hand-written expression.
const MAX_NEST_DEPTH: u32 = 256;

/// Build the `ShellError` for a general arithmetic evaluation failure.
/// All arithmetic errors are `ShellError::expansion`, so the POSIX §2.8.1
/// consequences table applies uniformly at every call site: a word-context
/// failure aborts a non-interactive shell, a heredoc-context failure is
/// converted to a redirection error at the redirect boundary (matching
/// dash/bash, which keep the shell alive there).
fn arith_err(msg: impl std::fmt::Display) -> ShellError {
    ShellError::expansion(
        ExpansionErrorKind::InvalidArithmetic,
        format!("arithmetic: {}", msg),
    )
}

/// `arith_err` variant for division/modulo by zero, carrying the dedicated
/// `ExpansionErrorKind::DivisionByZero` kind.
fn div_zero_err(msg: &str) -> ShellError {
    ShellError::expansion(
        ExpansionErrorKind::DivisionByZero,
        format!("arithmetic: {}", msg),
    )
}

/// Evaluate an arithmetic expression and return the result as a string.
/// Expands `$VAR`, `${VAR}`, `$(cmd)`, `` `cmd` ``, and nested `$((...))`
/// first (via the shared dollar-scanner), then parses and evaluates.
pub fn evaluate(env: &mut ShellEnv, expr: &str) -> crate::error::Result<String> {
    // A nested `$((...))` in the text recurses back here through
    // `expand_string` before any parsing happens, so `MAX_NEST_DEPTH`
    // never sees it: `$(($(($((…1…))))))` thousands deep (built by a
    // loop and `eval`ed) would overflow the native stack and abort the
    // shell, skipping the EXIT trap. Check the absolute stack left on
    // this recursion edge too.
    if crate::exec::stack_guard::descent_stack_exhausted() {
        return Err(arith_err("expression nested too deeply"));
    }
    // Step 1: expand dollar references. Expansions substitute their
    // actual (possibly empty) text — `$((1${x}2))` with `x` unset is 12,
    // matching bash/dash. Text without `$`, `\` or `` ` `` expands to
    // itself, so the common `$((i + 1))` skips the copy.
    let expanded: std::borrow::Cow<'_, str> =
        if expr.bytes().any(|b| matches!(b, b'$' | b'\\' | b'`')) {
            super::dollar::expand_string(env, expr)?.into()
        } else {
            expr.into()
        };

    // An entirely blank expression evaluates to 0 (bash/dash: `$(( ))`
    // and `$(($x))` with `x` unset/empty both print 0).
    if expanded.trim().is_empty() {
        return Ok("0".to_string());
    }

    // Step 2: parse and evaluate. Callers print the diagnostic (word
    // expansion via the ShellError path in `exec_command`, heredoc
    // expansion via the redirect-error path).
    // Only source text is worth caching: text produced by expansion
    // (`$((n + $1))` in a `shift` loop) is usually new every time, and
    // parsing it into a tree first would only add work.
    let cacheable = matches!(expanded, std::borrow::Cow::Borrowed(_));
    eval_text(env, &expanded, 0, 0, cacheable).map(|val| val.to_string())
}

/// Parse and evaluate the expanded expression `text` at variable
/// recursion depth `depth` and syntactic nesting `nest`. With `cacheable`
/// the text is evaluated from its cached tree (see [`cache`]), built on
/// first sight. Without it, and whenever no tree can be built (any syntax
/// error), the one-pass evaluator runs, so every diagnostic, and every
/// side effect preceding one, is exactly that evaluator's.
fn eval_text(
    env: &mut ShellEnv,
    text: &str,
    depth: u32,
    nest: u32,
    cacheable: bool,
) -> crate::error::Result<i64> {
    if cacheable && let Some(tree) = cache::lookup(text) {
        return cache::eval(env, &tree, depth, nest);
    }
    ArithParser::new(env, text, depth, nest).eval_full()
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::var::is_plain_decimal;
    use super::*;
    use crate::env::ShellEnv;

    fn env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn test_simple_number() {
        assert_eq!(evaluate(&mut env(), "42"), Ok("42".to_string()));
    }

    // ── PATH cache invalidation through arithmetic assignment (SP2) ──

    #[test]
    fn arith_simple_assignment_to_path_clears_utility_hash() {
        let mut e = env();
        e.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        assert_eq!(evaluate(&mut e, "PATH = 5"), Ok("5".to_string()));
        assert!(e.utility_hash.is_empty());
        assert_eq!(e.vars.get("PATH"), Some("5"));
    }

    #[test]
    fn arith_compound_assignment_to_path_clears_utility_hash() {
        let mut e = env();
        e.vars.set("PATH", "1").unwrap();
        e.utility_hash.insert(
            "foo".to_string(),
            crate::env::HashEntry::new(std::path::PathBuf::from("/bin/foo")),
        );
        assert_eq!(evaluate(&mut e, "PATH += 2"), Ok("3".to_string()));
        assert!(e.utility_hash.is_empty());
    }

    #[test]
    fn test_addition() {
        assert_eq!(evaluate(&mut env(), "1 + 2"), Ok("3".to_string()));
    }

    // ── LINENO (computed pseudo-variable, TODO PERF item 1) ──

    #[test]
    fn test_lineno_bare_identifier() {
        let mut e = env();
        e.exec.lineno = 9;
        assert_eq!(evaluate(&mut e, "LINENO"), Ok("9".to_string()));
    }

    #[test]
    fn test_lineno_dollar_form() {
        let mut e = env();
        e.exec.lineno = 9;
        assert_eq!(evaluate(&mut e, "$LINENO"), Ok("9".to_string()));
    }

    #[test]
    fn test_lineno_arith_does_not_persist_assignment() {
        let mut e = env();
        e.exec.lineno = 5;
        assert_eq!(evaluate(&mut e, "LINENO += 1"), Ok("6".to_string()));
        // Must not have materialized a real VarStore entry.
        assert_eq!(e.vars.get("LINENO"), None);
        // The computed pseudo-variable is unaffected by the arithmetic
        // "assignment" (bash: `$((LINENO+=1))` does not stick either).
        assert_eq!(e.exec.lineno, 5);
    }

    #[test]
    fn test_precedence() {
        assert_eq!(evaluate(&mut env(), "2 + 3 * 4"), Ok("14".to_string()));
    }

    #[test]
    fn test_parens() {
        assert_eq!(evaluate(&mut env(), "(2 + 3) * 4"), Ok("20".to_string()));
    }

    #[test]
    fn test_unary_minus() {
        assert_eq!(evaluate(&mut env(), "-5"), Ok("-5".to_string()));
    }

    #[test]
    fn test_comparison() {
        assert_eq!(evaluate(&mut env(), "3 > 2"), Ok("1".to_string()));
    }

    #[test]
    fn test_logical() {
        assert_eq!(evaluate(&mut env(), "1 && 0"), Ok("0".to_string()));
    }

    #[test]
    fn test_ternary() {
        assert_eq!(evaluate(&mut env(), "1 ? 10 : 20"), Ok("10".to_string()));
    }

    #[test]
    fn test_bitwise() {
        assert_eq!(evaluate(&mut env(), "5 & 3"), Ok("1".to_string()));
    }

    #[test]
    fn test_hex() {
        assert_eq!(evaluate(&mut env(), "0xFF"), Ok("255".to_string()));
    }

    #[test]
    fn test_octal() {
        assert_eq!(evaluate(&mut env(), "010"), Ok("8".to_string()));
    }

    #[test]
    fn test_variable() {
        let mut e = env();
        e.vars.set("x", "10").unwrap();
        assert_eq!(evaluate(&mut e, "x + 5"), Ok("15".to_string()));
    }

    #[test]
    fn test_int_min_div_neg_one_wraps() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(
            evaluate(&mut e, "x / -1"),
            Ok("-9223372036854775808".to_string())
        );
    }

    #[test]
    fn test_int_min_mod_neg_one_is_zero() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(evaluate(&mut e, "x % -1"), Ok("0".to_string()));
    }

    #[test]
    fn test_int_min_compound_div_assign_neg_one_wraps() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(
            evaluate(&mut e, "x /= -1"),
            Ok("-9223372036854775808".to_string())
        );
        assert_eq!(e.vars.get("x"), Some("-9223372036854775808"));
    }

    #[test]
    fn test_int_min_compound_mod_assign_neg_one_is_zero() {
        let mut e = env();
        e.vars.set("x", "-9223372036854775808").unwrap();
        assert_eq!(evaluate(&mut e, "x %= -1"), Ok("0".to_string()));
        assert_eq!(e.vars.get("x"), Some("0"));
    }

    #[test]
    fn test_dollar_variable() {
        let mut e = env();
        e.vars.set("x", "10").unwrap();
        assert_eq!(evaluate(&mut e, "$x + 5"), Ok("15".to_string()));
    }

    #[test]
    fn test_variable_assign() {
        let mut e = env();
        assert_eq!(evaluate(&mut e, "z = 5 + 3"), Ok("8".to_string()));
        assert_eq!(e.vars.get("z"), Some("8"));
    }

    #[test]
    fn test_positional_param_in_arith() {
        let mut e = ShellEnv::new("yosh", vec!["10".to_string(), "20".to_string()]);
        assert_eq!(evaluate(&mut e, "$1 + $2"), Ok("30".to_string()));
    }

    #[test]
    fn test_positional_param_zero() {
        let mut e = ShellEnv::new("yosh", vec!["5".to_string()]);
        // $0 is the shell name "yosh", non-numeric → defaults to 0
        assert_eq!(evaluate(&mut e, "$0"), Ok("0".to_string()));
    }

    #[test]
    fn test_special_param_hash_in_arith() {
        let mut e = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(evaluate(&mut e, "$# + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_special_param_question_in_arith() {
        let mut e = env();
        e.exec.last_exit_status = 42;
        assert_eq!(evaluate(&mut e, "$?"), Ok("42".to_string()));
    }

    #[test]
    fn test_braced_positional_param_in_arith() {
        let mut e = ShellEnv::new("yosh", vec!["100".to_string()]);
        assert_eq!(evaluate(&mut e, "${1} + 1"), Ok("101".to_string()));
    }

    #[test]
    fn test_unset_positional_param_defaults_to_zero() {
        let mut e = env();
        // No positional params set; $1 should default to 0
        assert_eq!(evaluate(&mut e, "$1 + 5"), Ok("5".to_string()));
    }

    // ── Unified dollar-scanner: full ${...} operator support ──

    #[test]
    fn test_braced_default_operator() {
        let mut e = env();
        assert_eq!(evaluate(&mut e, "${x:-3} + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_braced_length_operator() {
        let mut e = env();
        e.vars.set("a", "hello").unwrap();
        assert_eq!(evaluate(&mut e, "${#a}+1"), Ok("6".to_string()));
    }

    #[test]
    fn test_nested_arith_expansion() {
        let mut e = env();
        assert_eq!(evaluate(&mut e, "$((1+1)) + 1"), Ok("3".to_string()));
    }

    // ── Unified error channel: kind classification ──

    #[test]
    fn test_division_by_zero_has_dedicated_kind() {
        let err = evaluate(&mut env(), "1/0").unwrap_err();
        assert!(
            matches!(
                err.kind,
                crate::error::ShellErrorKind::Expansion(ExpansionErrorKind::DivisionByZero)
            ),
            "got: {:?}",
            err.kind
        );
        assert!(err.message.contains("division by zero"), "got: {err}");
        assert!(err.requires_noninteractive_exit());
    }

    #[test]
    fn test_syntax_error_is_invalid_arithmetic_kind() {
        let err = evaluate(&mut env(), "1 +").unwrap_err();
        assert!(
            matches!(
                err.kind,
                crate::error::ShellErrorKind::Expansion(ExpansionErrorKind::InvalidArithmetic)
            ),
            "got: {:?}",
            err.kind
        );
    }

    // ── Trailing-garbage detection (audit M5) ──

    #[test]
    fn test_trailing_garbage_is_syntax_error() {
        let mut e = env();
        let err = evaluate(&mut e, "1 2").unwrap_err();
        assert!(err.message.contains("syntax error"), "got: {err}");
    }

    #[test]
    fn test_trailing_whitespace_ok() {
        assert_eq!(evaluate(&mut env(), " 1 + 2  \n"), Ok("3".to_string()));
    }

    // ── Recursive variable-as-expression evaluation (audit M5) ──

    #[test]
    fn test_var_value_evaluated_recursively() {
        let mut e = env();
        e.vars.set("x", "1+2").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("3".to_string()));
        assert_eq!(evaluate(&mut e, "x + 1"), Ok("4".to_string()));
    }

    #[test]
    fn test_var_value_chain() {
        let mut e = env();
        e.vars.set("x", "y").unwrap();
        e.vars.set("y", "5").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("5".to_string()));
    }

    #[test]
    fn test_self_referential_var_errors_not_hangs() {
        let mut e = env();
        e.vars.set("x", "x").unwrap();
        let err = evaluate(&mut e, "x").unwrap_err();
        assert!(err.message.contains("recursion"), "got: {err}");
    }

    #[test]
    fn test_empty_var_value_is_zero() {
        let mut e = env();
        e.vars.set("x", "").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("0".to_string()));
    }

    #[test]
    fn test_compound_assign_recursive_current_value() {
        let mut e = env();
        e.vars.set("x", "1+2").unwrap();
        assert_eq!(evaluate(&mut e, "x += 1"), Ok("4".to_string()));
        assert_eq!(e.vars.get("x"), Some("4"));
    }

    // ── Variable values are C integer constants (octal / hex) ──

    #[test]
    fn test_var_value_octal_and_hex() {
        let mut e = env();
        e.vars.set("x", "010").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("8".to_string()));
        e.vars.set("x", "-010").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("-8".to_string()));
        e.vars.set("x", "0x1f").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("31".to_string()));
        e.vars.set("x", "0").unwrap();
        assert_eq!(evaluate(&mut e, "x"), Ok("0".to_string()));
        e.vars.set("x", "-7").unwrap();
        assert_eq!(evaluate(&mut e, "x + 1"), Ok("-6".to_string()));
    }

    #[test]
    fn test_var_value_invalid_octal_is_error() {
        let mut e = env();
        e.vars.set("x", "09").unwrap();
        assert!(evaluate(&mut e, "x").is_err());
    }

    #[test]
    fn test_is_plain_decimal() {
        for ok in ["0", "7", "42", "-7", "-0", "1234567890"] {
            assert!(is_plain_decimal(ok), "{ok}");
        }
        for not in ["", "-", "010", "-010", "09", "0x1f", "+5", "1+2", "00"] {
            assert!(!is_plain_decimal(not), "{not}");
        }
    }

    // ── Unevaluated operands have no side effects ──

    #[test]
    fn test_ternary_evaluates_only_selected_branch() {
        let mut e = env();
        e.vars.set("x", "0").unwrap();
        assert_eq!(evaluate(&mut e, "1 ? 2 : (x = 5)"), Ok("2".to_string()));
        assert_eq!(e.vars.get("x"), Some("0"));
        assert_eq!(evaluate(&mut e, "0 ? (x = 5) : 3"), Ok("3".to_string()));
        assert_eq!(e.vars.get("x"), Some("0"));
        assert_eq!(
            evaluate(&mut e, "0 ? (x = 5) : (x = 6)"),
            Ok("6".to_string())
        );
        assert_eq!(e.vars.get("x"), Some("6"));
    }

    #[test]
    fn test_logical_and_or_short_circuit() {
        let mut e = env();
        e.vars.set("x", "0").unwrap();
        assert_eq!(evaluate(&mut e, "0 && (x = 5)"), Ok("0".to_string()));
        assert_eq!(evaluate(&mut e, "1 || (x += 5)"), Ok("1".to_string()));
        assert_eq!(evaluate(&mut e, "0 && 1 / 0"), Ok("0".to_string()));
        assert_eq!(evaluate(&mut e, "1 || 1 % 0"), Ok("1".to_string()));
        assert_eq!(e.vars.get("x"), Some("0"));
        assert_eq!(evaluate(&mut e, "1 && (x = 5)"), Ok("1".to_string()));
        assert_eq!(e.vars.get("x"), Some("5"));
        // Syntax errors in a skipped operand are still reported.
        assert!(evaluate(&mut e, "0 && (1 +)").is_err());
    }

    // ── Readonly assignment is an error ──

    #[test]
    fn test_readonly_assignment_is_error() {
        let mut e = env();
        e.vars.set("r", "1").unwrap();
        e.vars.set_readonly("r");
        assert!(evaluate(&mut e, "r = 5").is_err());
        assert!(evaluate(&mut e, "r += 5").is_err());
        assert_eq!(e.vars.get("r"), Some("1"));
        // An unevaluated assignment to a readonly variable is harmless.
        assert_eq!(evaluate(&mut e, "0 && (r = 5)"), Ok("0".to_string()));
    }

    // ── Nesting depth limit ──

    #[test]
    fn test_deep_nesting_is_error_not_overflow() {
        // One nesting level takes ~13 KiB of debug-build stack, so run on
        // a thread with room for the full MAX_NEST_DEPTH.
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let mut e = env();
                let n = MAX_NEST_DEPTH as usize;
                let ok = format!("{}1{}", "(".repeat(n), ")".repeat(n));
                assert_eq!(evaluate(&mut e, &ok), Ok("1".to_string()));
                let deep = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
                let err = evaluate(&mut e, &deep).unwrap_err();
                assert!(err.to_string().contains("nested too deeply"), "{err}");
                let unary = format!("{}1", "-".repeat(100_000));
                assert!(evaluate(&mut e, &unary).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    // ── Diagnostics decode whole characters ──

    #[test]
    fn test_unexpected_non_ascii_char_reported_whole() {
        let err = evaluate(&mut env(), "é+1").unwrap_err();
        assert!(err.to_string().contains("'é'"), "{err}");
        let err = evaluate(&mut env(), "(1 日").unwrap_err();
        assert!(err.to_string().contains("'日'"), "{err}");
    }
}
