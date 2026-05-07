use super::command_checker::{CheckerEnv, CommandChecker};
use super::highlight::ColorSpan;

mod cache;
mod comment;
mod ctx;
mod expansion;
mod helpers;
mod normal;
mod quotes;
mod state;
mod word;

use cache::HighlightCache;
use ctx::ScanCtx;
use state::{ScanMode, ScannerState};

// ---------------------------------------------------------------------------
// HighlightScanner
// ---------------------------------------------------------------------------

/// Top-level scanner that produces colored spans for a line of shell input.
pub struct HighlightScanner {
    cache: HighlightCache,
    /// Accumulated state from prior PS2 lines: (accumulated_text, state).
    accumulated_state: Option<(String, ScannerState)>,
    checker: CommandChecker,
}

impl Default for HighlightScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl HighlightScanner {
    pub fn new() -> Self {
        Self {
            cache: HighlightCache::new(),
            accumulated_state: None,
            checker: CommandChecker::new(),
        }
    }

    /// Produce highlight spans for `current`.
    ///
    /// * `accumulated` – all prior PS2 lines concatenated (empty at PS1).
    /// * `current` – the current line buffer (as `&[char]`).
    /// * `checker_env` – environment for command-existence checks.
    pub fn scan(
        &mut self,
        accumulated: &str,
        current: &[char],
        checker_env: &CheckerEnv,
    ) -> Vec<ColorSpan> {
        if current.is_empty() {
            self.cache.clear();
            return Vec::new();
        }

        let is_ps1 = accumulated.is_empty();

        // Determine initial state -------------------------------------------------
        let init_state = if is_ps1 {
            ScannerState::new()
        } else {
            // Check cached accumulated state
            match &self.accumulated_state {
                Some((prev_acc, st)) if prev_acc == accumulated => st.clone(),
                _ => {
                    // Re-scan the accumulated text to get the ending state.
                    let acc_chars: Vec<char> = accumulated.chars().collect();
                    let mut st = ScannerState::new();
                    let _spans = self.scan_from(&acc_chars, 0, &mut st, checker_env);
                    self.accumulated_state = Some((accumulated.to_string(), st.clone()));
                    st
                }
            }
        };

        // Find rescan start -------------------------------------------------------
        let diff = self.cache.diff_pos(current);
        let (start_pos, mut state) = if diff == 0 || !is_ps1 {
            (0, init_state)
        } else if let Some((cp_pos, cp_state)) = self.cache.nearest_checkpoint(diff) {
            (cp_pos, cp_state)
        } else {
            (0, init_state)
        };

        // Scan from start_pos -----------------------------------------------------
        let mut spans = if start_pos > 0 {
            // Keep cached spans that end before start_pos.
            self.cache
                .prev_spans
                .iter()
                .filter(|sp| sp.end <= start_pos)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        let new_spans = self.scan_from(current, start_pos, &mut state, checker_env);
        spans.extend(new_spans);

        // Mark unclosed modes for PS1 ---------------------------------------------
        if is_ps1 {
            state::mark_unclosed_errors(&state, current.len(), &mut spans);
        }

        // Update cache ------------------------------------------------------------
        self.cache.prev_input = current.to_vec();
        self.cache.prev_spans = spans.clone();

        spans
    }

    // -----------------------------------------------------------------------
    // scan_from – scan chars[start_pos..] returning spans relative to chars
    // -----------------------------------------------------------------------

    fn scan_from(
        &mut self,
        chars: &[char],
        start_pos: usize,
        state: &mut ScannerState,
        checker_env: &CheckerEnv,
    ) -> Vec<ColorSpan> {
        let mut spans = Vec::new();
        let mut pos = start_pos;

        // Save checkpoints
        self.cache.checkpoints.retain(|(cp, _)| *cp < start_pos);
        if start_pos == 0 {
            self.cache.checkpoints.push((0, state.clone()));
        }

        while pos < chars.len() {
            // Periodically save checkpoints
            if pos > 0
                && pos.is_multiple_of(self.cache.checkpoint_interval)
                && !self.cache.checkpoints.iter().any(|(cp, _)| *cp == pos)
            {
                self.cache.checkpoints.push((pos, state.clone()));
            }

            let mut ctx = ScanCtx {
                input: chars,
                state,
                spans: &mut spans,
                checker: &mut self.checker,
            };
            pos = match ctx.state.current_mode().clone() {
                ScanMode::Normal => normal::scan_normal(&mut ctx, checker_env, pos),
                ScanMode::SingleQuote { start } => {
                    quotes::scan_single_quote(&mut ctx, checker_env, pos, start)
                }
                ScanMode::DoubleQuote { start } => {
                    quotes::scan_double_quote(&mut ctx, checker_env, pos, start)
                }
                ScanMode::DollarSingleQuote { start } => {
                    quotes::scan_dollar_single_quote(&mut ctx, checker_env, pos, start)
                }
                ScanMode::Parameter { start, braced } => {
                    expansion::scan_parameter(&mut ctx, checker_env, pos, start, braced)
                }
                ScanMode::ArithSub { start } => {
                    expansion::scan_arith_sub(&mut ctx, checker_env, pos, start)
                }
                ScanMode::Comment { start } => {
                    comment::scan_comment(&mut ctx, checker_env, pos, start)
                }
                ScanMode::CommandSub { .. } => {
                    // CommandSub itself doesn't scan — it pushes Normal which does the
                    // real scanning. When Normal pops, we detect the CommandSub below
                    // and pop it too. This is already handled in scan_normal (`)` case).
                    // If we somehow end up here, just pop.
                    ctx.state.pop_mode();
                    pos
                }
                ScanMode::Backtick { .. } => {
                    // Similar to CommandSub — handled by scan_normal.
                    ctx.state.pop_mode();
                    pos
                }
            };
        }

        spans
    }

}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::aliases::AliasStore;
    use crate::interactive::highlight::HighlightStyle;


    // ===================================================================
    // Scanner tests
    // ===================================================================

    fn test_scanner() -> HighlightScanner {
        HighlightScanner::new()
    }

    fn test_env() -> (String, AliasStore) {
        ("/usr/bin:/bin".to_string(), AliasStore::default())
    }

    fn scan_input(scanner: &mut HighlightScanner, input: &str) -> Vec<ColorSpan> {
        let (path, aliases) = test_env();
        let env = CheckerEnv {
            path: &path,
            aliases: &aliases,
        };
        let chars: Vec<char> = input.chars().collect();
        scanner.scan("", &chars, &env)
    }

    fn assert_span(
        spans: &[ColorSpan],
        idx: usize,
        start: usize,
        end: usize,
        style: HighlightStyle,
    ) {
        assert!(
            idx < spans.len(),
            "expected span at index {} but only {} spans exist: {:?}",
            idx,
            spans.len(),
            spans
        );
        let span = &spans[idx];
        assert_eq!(
            (span.start, span.end, &span.style),
            (start, end, &style),
            "span[{}] mismatch: got ({}, {}, {:?}), expected ({}, {}, {:?}). all spans: {:?}",
            idx,
            span.start,
            span.end,
            span.style,
            start,
            end,
            style,
            spans
        );
    }

    #[test]
    fn test_scan_simple_command() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "ls");
        assert_eq!(spans.len(), 1, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 2, HighlightStyle::CommandValid);
    }

    #[test]
    fn test_scan_invalid_command() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "xyzzy_no_such_cmd");
        assert_eq!(spans.len(), 1, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 17, HighlightStyle::CommandInvalid);
    }

    #[test]
    fn test_scan_command_with_args() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hello world");
        assert_eq!(spans.len(), 3, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 10, HighlightStyle::Default);
        assert_span(&spans, 2, 11, 16, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_pipe() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "ls | grep foo");
        assert_eq!(spans.len(), 4, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 2, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 3, 4, HighlightStyle::Operator);
        assert_span(&spans, 2, 5, 9, HighlightStyle::CommandValid);
        assert_span(&spans, 3, 10, 13, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_and_or() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "true && echo ok");
        assert_eq!(spans.len(), 4, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 7, HighlightStyle::Operator);
        assert_span(&spans, 2, 8, 12, HighlightStyle::CommandValid);
        assert_span(&spans, 3, 13, 15, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_semicolon() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo a; echo b");
        assert_eq!(spans.len(), 5, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 6, HighlightStyle::Default);
        assert_span(&spans, 2, 6, 7, HighlightStyle::Operator);
        assert_span(&spans, 3, 8, 12, HighlightStyle::CommandValid);
        assert_span(&spans, 4, 13, 14, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_keyword_if() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "if true; then echo hi; fi");
        // "if" → Keyword, "true" → CommandValid, ";" → Operator, "then" → Keyword,
        // "echo" → CommandValid, "hi" → Default, ";" → Operator, "fi" → Keyword
        assert_eq!(spans.len(), 8, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 2, HighlightStyle::Keyword);
        assert_span(&spans, 1, 3, 7, HighlightStyle::CommandValid);
        assert_span(&spans, 2, 7, 8, HighlightStyle::Operator);
        assert_span(&spans, 3, 9, 13, HighlightStyle::Keyword);
        assert_span(&spans, 4, 14, 18, HighlightStyle::CommandValid);
        assert_span(&spans, 5, 19, 21, HighlightStyle::Default);
        assert_span(&spans, 6, 21, 22, HighlightStyle::Operator);
        assert_span(&spans, 7, 23, 25, HighlightStyle::Keyword);
    }

    #[test]
    fn test_scan_comment() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hi # comment");
        // "echo" → CommandValid, "hi" → Default, "# comment" → Comment
        assert_eq!(spans.len(), 3, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 7, HighlightStyle::Default);
        assert_span(&spans, 2, 8, 17, HighlightStyle::Comment);
    }

    #[test]
    fn test_scan_comment_at_start() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "# full line comment");
        assert_eq!(spans.len(), 1, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 19, HighlightStyle::Comment);
    }

    #[test]
    fn test_scan_redirect() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hi > out.txt");
        // "echo" → CommandValid, "hi" → Default, ">" → Redirect, "out.txt" → Default
        assert_eq!(spans.len(), 4, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 7, HighlightStyle::Default);
        assert_span(&spans, 2, 8, 9, HighlightStyle::Redirect);
        assert_span(&spans, 3, 10, 17, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_redirect_append() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo hi >> out.txt");
        assert_eq!(spans.len(), 4, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 7, HighlightStyle::Default);
        assert_span(&spans, 2, 8, 10, HighlightStyle::Redirect);
        assert_span(&spans, 3, 11, 18, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_assignment() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "VAR=hello echo test");
        // "VAR=" → Assignment(0..4), "hello" → Default(4..9),
        // "echo" → CommandValid(10..14), "test" → Default(15..19)
        assert_eq!(spans.len(), 4, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 4, HighlightStyle::Assignment);
        assert_span(&spans, 1, 4, 9, HighlightStyle::Default);
        assert_span(&spans, 2, 10, 14, HighlightStyle::CommandValid);
        assert_span(&spans, 3, 15, 19, HighlightStyle::Default);
    }

    #[test]
    fn test_scan_background() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "sleep 1 &");
        // "sleep" → CommandValid, "1" → Default, "&" → Operator
        assert_eq!(spans.len(), 3, "spans: {:?}", spans);
        assert_span(&spans, 0, 0, 5, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 6, 7, HighlightStyle::Default);
        assert_span(&spans, 2, 8, 9, HighlightStyle::Operator);
    }

    // ── Error and PS2 tests ──────────────────────────────────────

    fn scan_ps2(
        scanner: &mut HighlightScanner,
        accumulated: &str,
        current: &str,
    ) -> Vec<ColorSpan> {
        let (path, aliases) = test_env();
        let env = CheckerEnv {
            path: &path,
            aliases: &aliases,
        };
        let chars: Vec<char> = current.chars().collect();
        scanner.scan(accumulated, &chars, &env)
    }

    #[test]
    fn test_scan_unclosed_single_quote_ps1() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo 'hello");
        let error_span = spans.iter().find(|s| s.style == HighlightStyle::Error);
        assert!(
            error_span.is_some(),
            "expected Error span for unclosed quote. Spans: {:?}",
            spans
        );
        let es = error_span.unwrap();
        assert_eq!(es.start, 5);
        assert_eq!(es.end, 11);
    }

    #[test]
    fn test_scan_unclosed_double_quote_ps1() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo \"hello");
        let error_span = spans.iter().find(|s| s.style == HighlightStyle::Error);
        assert!(
            error_span.is_some(),
            "expected Error span for unclosed double quote. Spans: {:?}",
            spans
        );
    }

    #[test]
    fn test_scan_unclosed_quote_ps2_not_error() {
        let mut scanner = test_scanner();
        let spans = scan_ps2(&mut scanner, "echo 'hello\n", "world'");
        let error_span = spans.iter().find(|s| s.style == HighlightStyle::Error);
        assert!(
            error_span.is_none(),
            "PS2 continuation should not show Error. Spans: {:?}",
            spans
        );
        let string_span = spans.iter().find(|s| s.style == HighlightStyle::String);
        assert!(
            string_span.is_some(),
            "expected String span in PS2. Spans: {:?}",
            spans
        );
    }

    #[test]
    fn test_scan_single_quoted_string() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo 'hello world'");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 18, HighlightStyle::String);
    }

    #[test]
    fn test_scan_double_quoted_string() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo \"hello\"");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 12, HighlightStyle::DoubleString);
    }

    #[test]
    fn test_scan_variable_in_double_quote() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo \"hi $USER\"");
        let var_span = spans.iter().find(|s| s.style == HighlightStyle::Variable);
        assert!(
            var_span.is_some(),
            "expected Variable span. Spans: {:?}",
            spans
        );
    }

    #[test]
    fn test_scan_variable_expansion() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo $HOME");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 10, HighlightStyle::Variable);
    }

    #[test]
    fn test_scan_braced_variable() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo ${USER}");
        assert_span(&spans, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 5, 12, HighlightStyle::Variable);
    }

    #[test]
    fn test_scan_command_substitution() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo $(ls)");
        let cs_spans: Vec<_> = spans
            .iter()
            .filter(|s| s.style == HighlightStyle::CommandSub)
            .collect();
        assert!(
            !cs_spans.is_empty(),
            "expected CommandSub spans. Spans: {:?}",
            spans
        );
    }

    #[test]
    fn test_scan_arith_sub() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "echo $((1+2))");
        let arith_spans: Vec<_> = spans
            .iter()
            .filter(|s| s.style == HighlightStyle::ArithSub)
            .collect();
        assert!(
            !arith_spans.is_empty(),
            "expected ArithSub spans. Spans: {:?}",
            spans
        );
    }

    #[test]
    fn test_scan_tilde() {
        let mut scanner = test_scanner();
        let spans = scan_input(&mut scanner, "cd ~/projects");
        assert_span(&spans, 0, 0, 2, HighlightStyle::CommandValid);
        assert_span(&spans, 1, 3, 4, HighlightStyle::Tilde);
    }

    // ── Incremental cache tests ──────────────────────────────────

    #[test]
    fn test_incremental_append() {
        let mut scanner = test_scanner();
        let spans1 = scan_input(&mut scanner, "ech");
        assert_span(&spans1, 0, 0, 3, HighlightStyle::CommandInvalid);

        let spans2 = scan_input(&mut scanner, "echo");
        assert_span(&spans2, 0, 0, 4, HighlightStyle::CommandValid);
    }

    #[test]
    fn test_incremental_backspace() {
        let mut scanner = test_scanner();
        let spans1 = scan_input(&mut scanner, "echo hello");
        assert_eq!(spans1.len(), 2);

        let spans2 = scan_input(&mut scanner, "echo hell");
        assert_eq!(spans2.len(), 2);
        assert_span(&spans2, 0, 0, 4, HighlightStyle::CommandValid);
        assert_span(&spans2, 1, 5, 9, HighlightStyle::Default);
    }

    #[test]
    fn test_incremental_full_rescan_on_history() {
        let mut scanner = test_scanner();
        let _spans1 = scan_input(&mut scanner, "echo hello");

        let spans2 = scan_input(&mut scanner, "ls -la");
        assert_span(&spans2, 0, 0, 2, HighlightStyle::CommandValid);
    }

    #[test]
    fn test_cache_cleared_on_empty() {
        let mut scanner = test_scanner();
        let _spans1 = scan_input(&mut scanner, "echo");
        let spans2 = scan_input(&mut scanner, "");
        assert!(spans2.is_empty());
    }

    #[test]
    fn test_accumulated_state_cached() {
        let mut scanner = test_scanner();
        let spans1 = scan_ps2(&mut scanner, "echo 'hello\n", "world'");
        let spans2 = scan_ps2(&mut scanner, "echo 'hello\n", "world'");
        assert_eq!(spans1, spans2);
    }
}
