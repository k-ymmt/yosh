//! The pattern-removal forms `${name%pat}`, `${name%%pat}`,
//! `${name#pat}` and `${name##pat}`, applied to an already expanded
//! value and pattern.

use crate::expand::pattern;
use crate::parser::ast::ParamExpr;

/// Apply the pattern removal of a `${name%pat}` / `${name%%pat}` /
/// `${name#pat}` / `${name##pat}` expansion to `value`, with the pattern
/// already expanded. Any other expression returns `value` unchanged.
pub(in crate::expand) fn apply_strip(param: &ParamExpr, value: &str, pat: &str) -> String {
    match param {
        ParamExpr::StripShortSuffix(..) => strip_suffix(value, pat, false),
        ParamExpr::StripLongSuffix(..) => strip_suffix(value, pat, true),
        ParamExpr::StripShortPrefix(..) => strip_prefix(value, pat, false),
        ParamExpr::StripLongPrefix(..) => strip_prefix(value, pat, true),
        _ => value.to_string(),
    }
}
/// Remove a suffix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
///
/// Fast paths for the common `${x%pattern}` idioms (substring search
/// instead of the general matcher):
///   - `pat` fully literal (no metachars) → a single `ends_with` check;
///     `longest`/`shortest` are equivalent since only one match is possible.
///   - `pat` = `<literal>*` (e.g. the `${x%.*}` idiom) → an anchored
///     `find`/`rfind` scan for the literal instead of testing every
///     candidate suffix boundary against the general matcher.
///   - Anything else (brackets, `?`, a `*` not at the pattern's end, or
///     multiple `*`s) takes the general linear scan below.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    match pattern::classify(pat) {
        pattern::PatternShape::Literal(lit) => {
            return match value.strip_suffix(lit.as_str()) {
                Some(rest) => rest.to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::LiteralThenStar(lit) => {
            if lit.is_empty() {
                // Pattern is just "*": longest strips everything, shortest
                // strips nothing (matches the general algorithm — see
                // task-2 brief verification harness).
                return if longest {
                    String::new()
                } else {
                    value.to_string()
                };
            }
            let idx = if longest {
                value.find(lit.as_str())
            } else {
                value.rfind(lit.as_str())
            };
            return match idx {
                Some(i) => value[..i].to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::StarThenLiteral(_) | pattern::PatternShape::General => {}
    }

    // General path: one linear scan for the shortest / longest matching
    // suffix (see `CompiledPattern::match_suffix`) instead of matching
    // every candidate suffix slice in turn, which is quadratic.
    match pattern::compile(pat).match_suffix(value, longest) {
        Some(start) => value[..start].to_string(),
        None => value.to_string(),
    }
}

/// Remove a prefix matching `pat` from `value`.
/// If `longest` is true, try the longest match; otherwise the shortest.
///
/// Fast paths mirror `strip_suffix` (see its doc comment):
///   - `pat` fully literal → a single `starts_with` check.
///   - `pat` = `*<literal>` (e.g. the `${x##*/}` idiom) → an anchored
///     `find`/`rfind` scan instead of the general matcher.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    match pattern::classify(pat) {
        pattern::PatternShape::Literal(lit) => {
            return match value.strip_prefix(lit.as_str()) {
                Some(rest) => rest.to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::StarThenLiteral(lit) => {
            if lit.is_empty() {
                // Pattern is just "*": longest strips everything, shortest
                // strips nothing.
                return if longest {
                    String::new()
                } else {
                    value.to_string()
                };
            }
            let idx = if longest {
                value.rfind(lit.as_str())
            } else {
                value.find(lit.as_str())
            };
            return match idx {
                Some(i) => value[i + lit.len()..].to_string(),
                None => value.to_string(),
            };
        }
        pattern::PatternShape::LiteralThenStar(_) | pattern::PatternShape::General => {}
    }

    // General path: one linear scan for the shortest / longest matching
    // prefix (see `CompiledPattern::match_prefix`).
    match pattern::compile(pat).match_prefix(value, longest) {
        Some(end) => value[end..].to_string(),
        None => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_prefix / strip_suffix fast-path unit tests (Task 2 PERF item 9) ──
    // Directly exercise the private helpers (module-internal test access) to
    // pin fast-path behavior independently of the ParamExpr plumbing tested
    // in `param::tests`.

    // -- Literal fast path --

    #[test]
    fn strip_suffix_literal_fast_path_match() {
        assert_eq!(strip_suffix("file.txt", ".txt", true), "file");
        assert_eq!(strip_suffix("file.txt", ".txt", false), "file");
    }

    #[test]
    fn strip_suffix_literal_fast_path_no_match() {
        assert_eq!(strip_suffix("file.txt", ".rs", true), "file.txt");
    }

    #[test]
    fn strip_prefix_literal_fast_path_match() {
        assert_eq!(strip_prefix("/a/b/c", "/a/", true), "b/c");
        assert_eq!(strip_prefix("/a/b/c", "/a/", false), "b/c");
    }

    #[test]
    fn strip_prefix_literal_fast_path_no_match() {
        assert_eq!(strip_prefix("/a/b/c", "/x/", true), "/a/b/c");
    }

    #[test]
    fn strip_literal_empty_pattern_is_identity() {
        // classify("") => Literal("") — matches at zero-length boundary,
        // stripping nothing.
        assert_eq!(strip_suffix("hello", "", true), "hello");
        assert_eq!(strip_prefix("hello", "", true), "hello");
    }

    #[test]
    fn strip_literal_pattern_longer_than_value_no_match() {
        assert_eq!(strip_suffix("ab", "abcdef", true), "ab");
        assert_eq!(strip_prefix("ab", "abcdef", true), "ab");
    }

    #[test]
    fn strip_literal_escaped_metachar_is_literal_not_wildcard() {
        // `\*` is a literal '*', not a wildcard — must not match unrelated
        // suffixes/prefixes, and must match a literal trailing/leading '*'.
        assert_eq!(strip_suffix("file*", "\\*", true), "file");
        assert_eq!(strip_suffix("file.txt", "\\*", true), "file.txt");
        assert_eq!(strip_prefix("*file", "\\*", true), "file");
    }

    // -- `*<literal>` / `<literal>*` idiom fast paths --

    #[test]
    fn strip_prefix_star_literal_longest_basename_idiom() {
        // ${x##*/} idiom: strip through the LAST '/'.
        assert_eq!(strip_prefix("/a/b/c.txt", "*/", true), "c.txt");
    }

    #[test]
    fn strip_prefix_star_literal_shortest_idiom() {
        // ${x#*/} idiom: strip through the FIRST '/'.
        assert_eq!(strip_prefix("/a/b/c.txt", "*/", false), "a/b/c.txt");
    }

    #[test]
    fn strip_prefix_star_literal_no_match() {
        assert_eq!(strip_prefix("noslash", "*/", true), "noslash");
    }

    #[test]
    fn strip_suffix_literal_star_shortest_extension_idiom() {
        // ${x%.*} idiom: strip from the LAST '.' onward.
        assert_eq!(strip_suffix("archive.tar.gz", ".*", false), "archive.tar");
    }

    #[test]
    fn strip_suffix_literal_star_longest_idiom() {
        // ${x%%.*} idiom: strip from the FIRST '.' onward.
        assert_eq!(strip_suffix("archive.tar.gz", ".*", true), "archive");
    }

    #[test]
    fn strip_suffix_literal_star_no_match() {
        assert_eq!(strip_suffix("noext", ".*", false), "noext");
    }

    #[test]
    fn strip_prefix_bare_star_longest_strips_everything() {
        assert_eq!(strip_prefix("anything", "*", true), "");
    }

    #[test]
    fn strip_prefix_bare_star_shortest_strips_nothing() {
        assert_eq!(strip_prefix("anything", "*", false), "anything");
    }

    #[test]
    fn strip_suffix_bare_star_longest_strips_everything() {
        assert_eq!(strip_suffix("anything", "*", true), "");
    }

    #[test]
    fn strip_suffix_bare_star_shortest_strips_nothing() {
        assert_eq!(strip_suffix("anything", "*", false), "anything");
    }

    // -- General fallback still correct for non-fast-path shapes --

    #[test]
    fn strip_prefix_question_mark_uses_general_path() {
        // '?' disqualifies the literal/star-literal fast paths.
        assert_eq!(strip_prefix("abc", "??", true), "c");
    }

    #[test]
    fn strip_suffix_bracket_uses_general_path() {
        assert_eq!(strip_suffix("file1", "[0-9]", true), "file");
    }

    #[test]
    fn strip_prefix_star_in_middle_uses_general_path() {
        // '*' not at an edge — StarThenLiteral/LiteralThenStar don't apply,
        // General handles "a*c" style patterns via full backtracking.
        assert_eq!(strip_prefix("axxxc rest", "a*c", true), " rest");
    }

    #[test]
    fn strip_suffix_multiple_stars_uses_general_path() {
        assert_eq!(strip_suffix("aXbYc", "a*b*c", true), "");
    }

    // -- Multibyte correctness for the new fast paths --

    #[test]
    fn strip_prefix_star_literal_multibyte() {
        assert_eq!(strip_prefix("日本/語/x", "*/", true), "x");
    }

    #[test]
    fn strip_suffix_literal_star_multibyte() {
        assert_eq!(strip_suffix("あ.い.う", ".*", false), "あ.い");
    }

    #[test]
    fn strip_literal_multibyte_pattern_fast_path() {
        assert_eq!(strip_prefix("日本語", "日本", true), "語");
        assert_eq!(strip_suffix("日本語", "語", true), "日本");
    }
}
