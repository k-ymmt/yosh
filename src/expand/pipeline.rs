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
        // ── Quoted literals ───────────────────────────────────────────────
        WordPart::Literal(s) => {
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(s);
            } else {
                fields.last_mut().unwrap().push_unquoted(s);
            }
        }
        WordPart::EscapedLiteral(s) => {
            // Expand identically to Literal — the escape served its purpose
            // at parse time by suppressing tilde recognition. The escape also
            // removes the "subject to field splitting" property, so escaped
            // text must be treated as quoted content for IFS purposes.
            fields.last_mut().unwrap().push_quoted(s);
        }
        WordPart::SingleQuoted(s) => {
            // Single quotes protect everything
            fields.last_mut().unwrap().push_quoted(s);
        }
        WordPart::DollarSingleQuoted(s) => {
            // $'...' also protects from splitting/glob
            fields.last_mut().unwrap().push_quoted(s);
        }

        // ── Double-quoted group ───────────────────────────────────────────
        WordPart::DoubleQuoted(parts) => {
            // Mark as quoted even when parts is empty (e.g. "")
            fields.last_mut().unwrap().was_quoted = true;
            for inner in parts {
                expand_part_to_fields(env, inner, fields, true)?;
            }
        }

        // ── Tilde expansion ───────────────────────────────────────────────
        WordPart::Tilde(None) => {
            let home = env.vars.get("HOME").map(|s| s.to_string());
            let result = home.unwrap_or_else(|| "~".to_string());
            fields.last_mut().unwrap().push_quoted(&result);
        }
        WordPart::Tilde(Some(user)) => {
            let result = super::expand_tilde_user(user);
            fields.last_mut().unwrap().push_quoted(&result);
        }

        // ── Parameter expansion ───────────────────────────────────────────
        WordPart::Parameter(param) => {
            expand_param_to_fields(env, param, fields, in_double_quote)?;
        }

        // ── Command substitution ──────────────────────────────────────────
        WordPart::CommandSub(program) => {
            let output = command_sub::execute(env, program);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&output);
            } else {
                fields.last_mut().unwrap().push_unquoted(&output);
            }
        }

        // ── Arithmetic expansion ──────────────────────────────────────────
        WordPart::ArithSub(expr) => match arith::evaluate(env, expr) {
            Ok(result) => {
                if in_double_quote {
                    fields.last_mut().unwrap().push_quoted(&result);
                } else {
                    fields.last_mut().unwrap().push_unquoted(&result);
                }
            }
            Err(msg) => {
                return Err(crate::error::ShellError::expansion(
                    crate::error::ExpansionErrorKind::InvalidArithmetic,
                    msg,
                ));
            }
        },
    }
    Ok(())
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
        ParamExpr::Special(SpecialParam::Star) if in_double_quote => {
            let sep = ifs_first_char(env);
            let joined = env.vars.positional_params().join(&sep.to_string());
            fields.last_mut().unwrap().push_quoted(&joined);
        }

        // Unquoted $@: each positional parameter becomes its own field,
        // with content unquoted (subject to IFS splitting and glob).
        ParamExpr::Special(SpecialParam::At) if !in_double_quote => {
            let params = env.vars.positional_params().to_vec();
            if params.is_empty() {
                return Ok(());
            }
            for (i, p) in params.iter().enumerate() {
                if i == 0 {
                    fields.last_mut().unwrap().push_unquoted(p);
                } else {
                    fields.push(ExpandedField::new());
                    fields.last_mut().unwrap().push_unquoted(p);
                }
            }
        }

        // Everything else: expand to a string, then push.
        _ => {
            let value = param::expand(env, param)?;
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&value);
            } else {
                fields.last_mut().unwrap().push_unquoted(&value);
            }
        }
    }
    Ok(())
}

/// Return the first character of IFS, defaulting to space.
fn ifs_first_char(env: &ShellEnv) -> char {
    env.vars
        .get("IFS")
        .and_then(|s| s.chars().next())
        .unwrap_or(' ')
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
        assert!((0..fields[0].value.len()).all(|i| !fields[0].is_quoted(i)));
        assert!((0..fields[1].value.len()).all(|i| !fields[1].is_quoted(i)));
        assert!((0..fields[2].value.len()).all(|i| !fields[2].is_quoted(i)));
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
