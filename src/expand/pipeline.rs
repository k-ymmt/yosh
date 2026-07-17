//! Field-producing core of the POSIX expansion pipeline.
//!
//! `expand_word_to_fields` is the entry point called by `expand::expand_word`
//! and `expand::expand_word_to_string`. It walks a `Word`'s parts, dispatching
//! each to a per-variant helper, and accumulates `ExpandedField` values that
//! the public API then runs through field-splitting, pathname expansion, and
//! quote removal.

use super::ExpandedField;
use super::{arith, command_sub, param};
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

/// Expand a `Word` into a list of `ExpandedField`s (before field splitting).
pub(super) fn expand_word_to_fields(
    env: &mut ShellEnv,
    word: &Word,
) -> crate::error::Result<Vec<ExpandedField>> {
    let mut fields = vec![ExpandedField::new()];
    for part in &word.parts {
        expand_part_to_fields(env, part, &mut fields, false)?;
    }
    Ok(fields)
}

/// Expand one `WordPart`, appending into `fields`.
/// `in_double_quote` is true when we are inside `DoubleQuoted(...)`.
fn expand_part_to_fields(
    env: &mut ShellEnv,
    part: &WordPart,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match part {
        WordPart::Literal(s) => expand_part_literal(s, fields, in_double_quote),
        WordPart::EscapedLiteral(s)
        | WordPart::SingleQuoted(s)
        | WordPart::DollarSingleQuoted(s) => expand_part_quoted_literal(s, fields),
        WordPart::DoubleQuoted(parts) => expand_part_double_quoted(env, parts, fields)?,
        WordPart::Tilde(user) => expand_part_tilde(env, user.as_deref(), fields),
        WordPart::Parameter(p) => expand_part_parameter(env, p, fields, in_double_quote)?,
        WordPart::CommandSub(program) => {
            expand_part_command_sub(env, program, fields, in_double_quote)
        }
        WordPart::ArithSub(expr) => expand_part_arith_sub(env, expr, fields, in_double_quote)?,
    }
    Ok(())
}

fn expand_part_literal(s: &str, fields: &mut [ExpandedField], in_double_quote: bool) {
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(s);
    } else {
        fields.last_mut().unwrap().push_literal(s);
    }
}

/// `EscapedLiteral`, `SingleQuoted`, and `DollarSingleQuoted` all push their
/// text as quoted (protected from field splitting and pathname expansion).
/// They differ only in their parser-level meaning, not their expansion behavior.
fn expand_part_quoted_literal(s: &str, fields: &mut [ExpandedField]) {
    fields.last_mut().unwrap().push_quoted(s);
}

fn expand_part_double_quoted(
    env: &mut ShellEnv,
    parts: &[WordPart],
    fields: &mut Vec<ExpandedField>,
) -> crate::error::Result<()> {
    fields.last_mut().unwrap().was_quoted = true;
    for inner in parts {
        expand_part_to_fields(env, inner, fields, true)?;
    }
    Ok(())
}

fn expand_part_tilde(env: &mut ShellEnv, user: Option<&str>, fields: &mut [ExpandedField]) {
    let result = match user {
        None => env
            .vars
            .get("HOME")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "~".to_string()),
        Some(name) => super::tilde::expand_tilde_user(name),
    };
    fields.last_mut().unwrap().push_quoted(&result);
}

fn expand_part_parameter(
    env: &mut ShellEnv,
    param: &ParamExpr,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    expand_param_to_fields(env, param, fields, in_double_quote)
}

fn expand_part_command_sub(
    env: &mut ShellEnv,
    program: &crate::parser::ast::Program,
    fields: &mut [ExpandedField],
    in_double_quote: bool,
) {
    let output = command_sub::execute(env, program);
    if in_double_quote {
        fields.last_mut().unwrap().push_quoted(&output);
    } else {
        fields.last_mut().unwrap().push_expanded(&output);
    }
}

fn expand_part_arith_sub(
    env: &mut ShellEnv,
    expr: &str,
    fields: &mut [ExpandedField],
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match arith::evaluate(env, expr) {
        Ok(result) => {
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&result);
            } else {
                fields.last_mut().unwrap().push_expanded(&result);
            }
            Ok(())
        }
        Err(msg) => Err(crate::error::ShellError::expansion(
            crate::error::ExpansionErrorKind::InvalidArithmetic,
            msg,
        )),
    }
}

/// Expand a `ParamExpr` into `fields`.
fn expand_param_to_fields(
    env: &mut ShellEnv,
    param: &ParamExpr,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match param {
        // "$@" inside double quotes: each positional parameter becomes its own field.
        ParamExpr::Special(SpecialParam::At) if in_double_quote => {
            let params = env.vars.positional_params().to_vec();
            if params.is_empty() {
                // "$@" with no params → produces nothing (not even an empty field)
                // Remove the last (empty) field if it is empty.
                if fields.last().map(|f| f.is_empty()).unwrap_or(false) {
                    fields.pop();
                }
                return Ok(());
            }
            for (i, p) in params.iter().enumerate() {
                if i == 0 {
                    fields.last_mut().unwrap().push_quoted(p);
                } else {
                    fields.push(ExpandedField::new());
                    fields.last_mut().unwrap().push_quoted(p);
                }
            }
        }

        // "$*" inside double quotes: join all positional params with IFS[0].
        // A set-but-null IFS joins with no separator (POSIX §2.5.2).
        ParamExpr::Special(SpecialParam::Star) if in_double_quote => {
            let sep = ifs_join_separator(env);
            let joined = env.vars.positional_params().join(&sep);
            fields.last_mut().unwrap().push_quoted(&joined);
        }

        // Unquoted $@ / $*: each positional parameter becomes its own field,
        // with content unquoted (subject to IFS splitting and glob). POSIX
        // §2.5.2 gives `*` the same initial one-field-per-parameter shape as
        // `@` outside double quotes; joining with IFS[0] applies only to the
        // quoted "$*" form (verified against bash and dash).
        ParamExpr::Special(SpecialParam::At | SpecialParam::Star) if !in_double_quote => {
            let params = env.vars.positional_params().to_vec();
            if params.is_empty() {
                return Ok(());
            }
            for (i, p) in params.iter().enumerate() {
                if i == 0 {
                    fields.last_mut().unwrap().push_expanded(p);
                } else {
                    fields.push(ExpandedField::new());
                    fields.last_mut().unwrap().push_expanded(p);
                }
            }
        }

        // ${name:-word} / ${name-word}: substituting `word` must preserve its
        // quote structure — an unquoted ${x:-"a b"} yields the single field
        // `a b` (POSIX §2.6.2), so the word's parts are expanded through the
        // normal part pipeline instead of being flattened to a string.
        ParamExpr::Default {
            name,
            word,
            null_check,
        } => {
            let val = param::lookup_var(env, name);
            if param::is_unset_or_null_inner(&val, *null_check) {
                if let Some(w) = word.as_ref() {
                    for part in &w.parts {
                        expand_part_to_fields(env, part, fields, in_double_quote)?;
                    }
                }
            } else {
                push_param_value(fields, &val.unwrap_or_default(), in_double_quote);
            }
        }

        // ${name:+word} / ${name+word}: same quote-preserving substitution.
        ParamExpr::Alt {
            name,
            word,
            null_check,
        } => {
            let val = param::lookup_var(env, name);
            if !param::is_unset_or_null_inner(&val, *null_check) {
                if let Some(w) = word.as_ref() {
                    for part in &w.parts {
                        expand_part_to_fields(env, part, fields, in_double_quote)?;
                    }
                }
            }
        }

        // ${name:=word} / ${name=word}: assign the quote-removed expansion of
        // `word`, but substitute with the word's quote structure preserved
        // (matches bash; dash re-splits the assigned value here).
        ParamExpr::Assign {
            name,
            word,
            null_check,
        } => {
            let val = param::lookup_var(env, name);
            if param::is_unset_or_null_inner(&val, *null_check) {
                // POSIX §2.6.2: attempting to assign a positional or special
                // parameter with ${name=word} is an error.
                if param::is_unassignable_param(name) {
                    eprintln!("yosh: {}: cannot assign in this way", name);
                    env.exec.last_exit_status = 1;
                    env.exec.flow_control = Some(crate::env::FlowControl::Return(1));
                    return Ok(());
                }
                let mut sub = vec![ExpandedField::new()];
                if let Some(w) = word.as_ref() {
                    for part in &w.parts {
                        expand_part_to_fields(env, part, &mut sub, in_double_quote)?;
                    }
                }
                let new_val = sub
                    .iter()
                    .map(|f| f.value.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                // LINENO is a computed pseudo-variable (see `param::lookup_var`);
                // assigning it would resurrect a real `VarStore` entry.
                if name != "LINENO" {
                    let _ = env.vars.set(name, &new_val);
                }
                let mut sub = sub.into_iter();
                if let Some(first) = sub.next() {
                    fields.last_mut().unwrap().append_field(&first);
                }
                fields.extend(sub);
            } else {
                push_param_value(fields, &val.unwrap_or_default(), in_double_quote);
            }
        }

        // Everything else: expand to a string, then push.
        _ => {
            let value = param::expand(env, param)?;
            push_param_value(fields, &value, in_double_quote);
        }
    }
    Ok(())
}

/// Push a parameter's value into the current field: quoted context protects
/// it, unquoted context leaves it split- and glob-subject.
fn push_param_value(fields: &mut [ExpandedField], value: &str, in_double_quote: bool) {
    let f = fields.last_mut().unwrap();
    if in_double_quote {
        f.push_quoted(value);
    } else {
        f.push_expanded(value);
    }
}

/// Return the `"$*"` join separator: the first character of IFS.
/// Unset IFS joins with a space; set-but-null IFS joins with nothing
/// (POSIX §2.5.2).
fn ifs_join_separator(env: &ShellEnv) -> String {
    match env.vars.get("IFS") {
        None => " ".to_string(),
        Some(s) => s.chars().next().map(String::from).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;
    use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

    #[test]
    fn test_unquoted_dollar_at_splits_per_param() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 3, "expected 3 fields, got {:?}", fields);
        assert_eq!(fields[0].value, "a");
        assert_eq!(fields[1].value, "b");
        assert_eq!(fields[2].value, "c");
        assert!((0..fields[0].value.len()).all(|i| !fields[0].is_split_protected(i)));
        assert!((0..fields[1].value.len()).all(|i| !fields[1].is_split_protected(i)));
        assert!((0..fields[2].value.len()).all(|i| !fields[2].is_split_protected(i)));
    }

    #[test]
    fn test_unquoted_dollar_star_splits_per_param() {
        let mut env = ShellEnv::new(
            "yosh",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::Star))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 3, "expected 3 fields, got {:?}", fields);
        assert_eq!(fields[0].value, "a");
        assert_eq!(fields[1].value, "b");
        assert_eq!(fields[2].value, "c");
    }

    #[test]
    fn test_default_word_quoted_part_is_protected() {
        // ${x:-"a b"} with x unset: the quoted word must be split-protected.
        let mut env = ShellEnv::new("yosh", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "UNSET_XYZ".to_string(),
                word: Some(Word {
                    parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                        "a b".to_string(),
                    )])],
                }),
                null_check: true,
            })],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "a b");
        assert!((0..fields[0].value.len()).all(|i| fields[0].is_split_protected(i)));
    }

    #[test]
    fn test_default_unquoted_var_word_still_splits() {
        // ${x:-$y} with y="p q": the substituted value stays split-subject.
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("y", "p q").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Default {
                name: "UNSET_XYZ".to_string(),
                word: Some(Word {
                    parts: vec![WordPart::Parameter(ParamExpr::Simple("y".to_string()))],
                }),
                null_check: true,
            })],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "p q");
        assert!((0..fields[0].value.len()).all(|i| !fields[0].is_split_protected(i)));
    }

    #[test]
    fn test_assign_quoted_word_assigns_and_protects() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Assign {
                name: "NEWVAR".to_string(),
                word: Some(Word {
                    parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                        "a b".to_string(),
                    )])],
                }),
                null_check: true,
            })],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "a b");
        assert!((0..fields[0].value.len()).all(|i| fields[0].is_split_protected(i)));
        assert_eq!(env.vars.get("NEWVAR"), Some("a b"));
    }

    #[test]
    fn test_alt_quoted_word_is_protected() {
        let mut env = ShellEnv::new("yosh", vec![]);
        env.vars.set("SETVAR", "1").unwrap();
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Alt {
                name: "SETVAR".to_string(),
                word: Some(Word {
                    parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                        "a b".to_string(),
                    )])],
                }),
                null_check: true,
            })],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "a b");
        assert!((0..fields[0].value.len()).all(|i| fields[0].is_split_protected(i)));
    }

    #[test]
    fn test_unquoted_dollar_at_empty_produces_nothing() {
        let mut env = ShellEnv::new("yosh", vec![]);
        let word = Word {
            parts: vec![WordPart::Parameter(ParamExpr::Special(SpecialParam::At))],
        };
        let fields = expand_word_to_fields(&mut env, &word).unwrap();
        assert!(
            fields.len() <= 1,
            "expected 0 or 1 fields, got {:?}",
            fields
        );
    }
}
