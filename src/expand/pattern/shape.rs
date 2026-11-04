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
pub(crate) fn classify(pat: &str) -> PatternShape {
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
