//! Pattern-shape classification for the literal fast paths of the
//! `${v#pat}` family.

use super::{PatternToken, parse_pattern};

/// Classification of a pattern's shape, used by callers (e.g.
/// `param::strip_prefix` / `strip_suffix`) to pick an O(1)/O(n) anchored
/// fast path instead of the general O(n) boundary-scan + backtracking
/// matcher. Only patterns built entirely from literal characters (ordinary
/// chars and `\x` escapes — no `*`, `?`, or `[...]`) qualify; anything else
/// falls back to `General` so behavior for bracket/`?`-bearing patterns is
/// unchanged.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) enum PatternShape {
    /// No metacharacters at all — an exact literal string.
    Literal(String),
    /// A single leading `*` followed by a literal remainder, e.g. the
    /// `${x##*/}` / `${x#*/}` idiom's pattern `*/`.
    StarThenLiteral(String),
    /// A literal followed by a single trailing `*`, e.g. the
    /// `${x%.*}` / `${x%%.*}` idiom's pattern `.*`.
    LiteralThenStar(String),
    /// Anything else (multiple `*`, `?`, bracket expressions, or a `*` not
    /// at an end) — callers must use the general matcher.
    General,
}

/// Classify `pat`'s shape for fast-path dispatch. See `PatternShape`.
///
/// Scans `pat` directly instead of building the token vector that
/// [`classify_tokens`] (the reference) works on: a pattern with a `[`
/// (a bracket expression, or a literal `[` when malformed) takes the
/// reference path; otherwise `\x` is the literal `x`, a `?` makes the
/// pattern `General`, and at most one `*` may stand at either end.
pub(crate) fn classify(pat: &str) -> PatternShape {
    if pat.contains('[') {
        return classify_tokens(pat);
    }
    let mut lit = String::with_capacity(pat.len());
    // Token index of the only `*`, and the number of tokens so far.
    let mut star_at = None;
    let mut ntokens = 0;
    let mut chars = pat.chars();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if star_at.is_some() {
                    return PatternShape::General;
                }
                star_at = Some(ntokens);
            }
            '?' => return PatternShape::General,
            '\\' => lit.push(chars.next().unwrap_or('\\')),
            c => lit.push(c),
        }
        ntokens += 1;
    }
    match star_at {
        None => PatternShape::Literal(lit),
        Some(0) => PatternShape::StarThenLiteral(lit),
        Some(i) if i + 1 == ntokens => PatternShape::LiteralThenStar(lit),
        Some(_) => PatternShape::General,
    }
}

/// `classify` over the parsed pattern tokens: the reference
/// implementation, used for patterns with a `[`.
fn classify_tokens(pat: &str) -> PatternShape {
    let tokens = parse_pattern(pat);
    let Some(lit) = literal_run(&tokens) else {
        return PatternShape::General;
    };
    let has_leading_star = matches!(tokens.first(), Some(PatternToken::Star));
    let has_trailing_star = matches!(tokens.last(), Some(PatternToken::Star));
    if has_leading_star {
        PatternShape::StarThenLiteral(lit)
    } else if has_trailing_star {
        PatternShape::LiteralThenStar(lit)
    } else {
        // No `*` at all (empty tokens counts as the empty literal too).
        PatternShape::Literal(lit)
    }
}

/// If `tokens` is either all-`Literal` or a single leading/trailing `Star`
/// plus all-`Literal` otherwise, return the concatenated literal value.
/// Returns `None` for anything containing `Any`, `Bracket`, more than one
/// `Star`, or a `Star` that isn't strictly at the first/last position.
fn literal_run(tokens: &[PatternToken]) -> Option<String> {
    let star_count = tokens
        .iter()
        .filter(|t| matches!(t, PatternToken::Star))
        .count();
    if star_count > 1 {
        return None;
    }
    if star_count == 1 {
        let is_edge_star = matches!(tokens.first(), Some(PatternToken::Star))
            || matches!(tokens.last(), Some(PatternToken::Star));
        if !is_edge_star {
            return None;
        }
    }

    let mut lit = String::new();
    for t in tokens {
        match t {
            PatternToken::Literal(c) => lit.push(*c),
            PatternToken::Star => {} // skip; position already validated above
            PatternToken::Any | PatternToken::Bracket { .. } => return None,
        }
    }
    Some(lit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_scan_matches_token_reference() {
        let pats = [
            "", "*", "**", "?", "a", "abc", "*a", "a*", "*ab*", "a*b", ".*", "*/", r"\*", r"\*a",
            r"a\*", r"*\*", r"\?", r"\", r"a\", r"\\", r"\\*", r"*\", "a?b", "*?", "日本*", "*語",
            "[ab]", "a[b", "*[x]", "[", r"\[a*", "a]*", "]", "*a*", r"\**", r"*a\",
        ];
        for pat in pats {
            assert_eq!(classify(pat), classify_tokens(pat), "pattern {pat:?}");
        }
    }
}
