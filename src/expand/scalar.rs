//! The scalar fast path: a word that expands to exactly one string
//! (literal and quoted text plus scalar expansions) skips the per-byte
//! field machinery. See [`scalar_word`].

use std::borrow::Cow;

use super::{arith, command_sub, param};
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};

/// `true` for a parameter expansion that always yields one scalar
/// string (everything but `$@`/`$*` and the modifier forms, whose words
/// keep their own quote structure).
fn is_scalar_param(p: &ParamExpr) -> bool {
    match p {
        ParamExpr::Simple(_) | ParamExpr::Positional(_) => true,
        ParamExpr::Special(sp) => !matches!(sp, SpecialParam::At | SpecialParam::Star),
        _ => false,
    }
}

/// Whether `part` can take the scalar fast path (see [`scalar_word`]).
/// Unquoted expansions qualify only in a string context (no field
/// splitting there); an unquoted literal holding a pathname-expansion
/// character qualifies only when `allow_glob_meta` is set.
fn scalar_part_ok(part: &WordPart, quoted: bool, string_ctx: bool, allow_glob_meta: bool) -> bool {
    match part {
        WordPart::Literal(s) => {
            quoted || allow_glob_meta || !s.bytes().any(|b| matches!(b, b'*' | b'?' | b'['))
        }
        WordPart::EscapedLiteral(_)
        | WordPart::SingleQuoted(_)
        | WordPart::DollarSingleQuoted(_) => true,
        WordPart::Parameter(p) => (quoted || string_ctx) && is_scalar_param(p),
        WordPart::ArithSub(_) | WordPart::CommandSub(_) => quoted || string_ctx,
        WordPart::DoubleQuoted(inner) => {
            !quoted
                && inner
                    .iter()
                    .all(|p| scalar_part_ok(p, true, string_ctx, allow_glob_meta))
        }
        WordPart::Tilde(_) => false,
    }
}

/// Append the expansion of a part accepted by [`scalar_part_ok`].
fn push_scalar_part(
    env: &mut ShellEnv,
    part: &WordPart,
    out: &mut String,
) -> crate::error::Result<()> {
    match part {
        WordPart::Literal(s)
        | WordPart::EscapedLiteral(s)
        | WordPart::SingleQuoted(s)
        | WordPart::DollarSingleQuoted(s) => out.push_str(s),
        WordPart::Parameter(p) => match param::set_scalar_value(env, p) {
            // A set `$name` / `$1`: copied once, straight from the
            // variable (`out.push_str` on an empty `out` allocates just
            // what a clone would).
            Some(Cow::Borrowed(value)) => out.push_str(value),
            Some(Cow::Owned(value)) => push_owned(out, value),
            None => push_owned(out, param::expand(env, p)?),
        },
        WordPart::ArithSub(expr) => push_owned(out, arith::evaluate(env, expr)?),
        WordPart::CommandSub(program) => push_owned(out, command_sub::execute(env, program)),
        WordPart::DoubleQuoted(inner) => {
            for p in inner {
                push_scalar_part(env, p, out)?;
            }
        }
        WordPart::Tilde(_) => unreachable!("rejected by scalar_part_ok"),
    }
    Ok(())
}

/// Append `s`, reusing its buffer when `out` is still empty.
fn push_owned(out: &mut String, s: String) {
    if out.is_empty() {
        *out = s;
    } else {
        out.push_str(&s);
    }
}

/// Fast path for a word that expands to exactly one string — the
/// common case for command names, arguments such as `"$x"` or `-lt`,
/// and assignment values: literal and quoted text plus quoted (or, in a
/// string context, unquoted) scalar expansions. Such a word never
/// splits, and an unquoted literal part only matters for pathname
/// expansion, so the per-byte field machinery can be skipped. Parts
/// are expanded in order with the same functions the field pipeline
/// uses. Returns `None` when the word needs the full pipeline, else the
/// text and whether any part was quoted.
pub(super) fn scalar_word(
    env: &mut ShellEnv,
    word: &Word,
    string_ctx: bool,
) -> Option<crate::error::Result<(String, bool)>> {
    let allow_glob_meta = string_ctx || env.mode.options.noglob;
    if !word
        .parts
        .iter()
        .all(|p| scalar_part_ok(p, false, string_ctx, allow_glob_meta))
    {
        return None;
    }
    let mut text = String::new();
    let mut quoted = false;
    for part in &word.parts {
        quoted |= !matches!(
            part,
            WordPart::Literal(_)
                | WordPart::Parameter(_)
                | WordPart::ArithSub(_)
                | WordPart::CommandSub(_)
        );
        if let Err(e) = push_scalar_part(env, part, &mut text) {
            return Some(Err(e));
        }
    }
    Some(Ok((text, quoted)))
}
