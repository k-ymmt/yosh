//! Glob-style argv allowlist patterns for the `commands:exec` capability.
//!
//! See `docs/superpowers/specs/2026-04-29-plugin-commands-exec-capability-design.md` §4.

/// A parsed allowlist pattern. Matches against an argv slice of any
/// `AsRef<str>` type (e.g. `&[String]`, `&[&str]`, `&[Cow<'_, str>]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPattern {
    pub tokens: Vec<String>,
    pub has_glob_suffix: bool,
}

impl CommandPattern {
    /// Parse a single pattern string. Tokens are whitespace-separated.
    /// A trailing `:*` (no whitespace before it) marks the pattern as
    /// a prefix match; otherwise the pattern is exact-length.
    ///
    /// Errors:
    /// * empty / whitespace-only input
    /// * a lone `:*` with no preceding tokens
    pub fn parse(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("empty pattern".to_string());
        }

        let (body, has_glob_suffix) = if let Some(stripped) = trimmed.strip_suffix(":*") {
            (stripped.trim_end(), true)
        } else {
            (trimmed, false)
        };

        if body.is_empty() {
            return Err("pattern has `:*` but no tokens".to_string());
        }

        if body.contains(":*") {
            return Err(
                "`:*` may only appear as a trailing suffix on the whole pattern".to_string(),
            );
        }

        let tokens: Vec<String> = body.split_whitespace().map(|t| t.to_string()).collect();

        Ok(CommandPattern {
            tokens,
            has_glob_suffix,
        })
    }

    /// Match this pattern against an argv slice (`[program, arg1, arg2, ...]`).
    ///
    /// Generic over `S: AsRef<str>` so callers can pass `&[String]`
    /// (existing) or `&[&str]` / `&[Cow<'_, str>]` (the canonical-ABI
    /// borrow path in `host_commands_exec`).
    pub fn matches<S: AsRef<str>>(&self, argv: &[S]) -> bool {
        if self.has_glob_suffix {
            argv.len() >= self.tokens.len()
                && self
                    .tokens
                    .iter()
                    .zip(argv)
                    .all(|(p, a)| p.as_str() == a.as_ref())
        } else {
            argv.len() == self.tokens.len()
                && self
                    .tokens
                    .iter()
                    .zip(argv)
                    .all(|(p, a)| p.as_str() == a.as_ref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_glob_suffix_separates_tokens() {
        let p = CommandPattern::parse("git log:*").unwrap();
        assert_eq!(p.tokens, vec!["git".to_string(), "log".to_string()]);
        assert!(p.has_glob_suffix);
    }

    #[test]
    fn parse_no_suffix_is_exact() {
        let p = CommandPattern::parse("git log").unwrap();
        assert_eq!(p.tokens, vec!["git".to_string(), "log".to_string()]);
        assert!(!p.has_glob_suffix);
    }

    #[test]
    fn parse_empty_string_errors() {
        assert!(CommandPattern::parse("").is_err());
        assert!(CommandPattern::parse("   ").is_err());
    }

    #[test]
    fn parse_lone_glob_suffix_errors() {
        assert!(CommandPattern::parse(":*").is_err());
        assert!(CommandPattern::parse("  :*").is_err());
    }

    #[test]
    fn match_glob_suffix_zero_extra() {
        let p = CommandPattern::parse("git:*").unwrap();
        assert!(p.matches(&["git".to_string()]));
    }

    #[test]
    fn match_glob_suffix_many_extra() {
        let p = CommandPattern::parse("git:*").unwrap();
        assert!(p.matches(&["git".to_string(), "log".to_string(), "-p".to_string(),]));
    }

    #[test]
    fn match_exact_requires_equal_length() {
        let p = CommandPattern::parse("git status").unwrap();
        assert!(p.matches(&["git".to_string(), "status".to_string()]));
        assert!(!p.matches(&[
            "git".to_string(),
            "status".to_string(),
            "--porcelain".to_string()
        ]));
        assert!(!p.matches(&["git".to_string()]));
    }

    #[test]
    fn match_literal_compare() {
        let p = CommandPattern::parse("git:*").unwrap();
        assert!(!p.matches(&["/usr/bin/git".to_string(), "status".to_string()]));
    }

    #[test]
    fn parse_mid_string_glob_suffix_errors() {
        assert!(CommandPattern::parse("git:*:*").is_err());
        assert!(CommandPattern::parse("git:* status:*").is_err());
        assert!(CommandPattern::parse("foo:* bar").is_err());
    }

    #[test]
    fn match_glob_suffix_subcommand_lock() {
        let p = CommandPattern::parse("git status:*").unwrap();
        assert!(p.matches(&["git".to_string(), "status".to_string()]));
        assert!(p.matches(&[
            "git".to_string(),
            "status".to_string(),
            "--porcelain".to_string()
        ]));
        assert!(!p.matches(&["git".to_string(), "log".to_string()]));
    }

    #[test]
    fn matches_accepts_str_slice_argv() {
        // Locks down the §4.1 follow-up generalization: matcher must
        // accept &[&str] (used by host_commands_exec after the borrow
        // rollout) as well as &[String] (existing callsites).
        let p = CommandPattern::parse("/bin/echo:*").unwrap();
        let argv: &[&str] = &["/bin/echo", "a", "b"];
        assert!(p.matches(argv));
    }
}
