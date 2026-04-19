use super::command_checker::{CheckerEnv, CommandChecker, CommandExistence};
use super::highlight::{ColorSpan, HighlightStyle};

// ---------------------------------------------------------------------------
// ScanMode – the mode stack entries for the highlight scanner
// ---------------------------------------------------------------------------

/// Each mode represents a different parsing context inside the input line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScanMode {
    Normal,
    SingleQuote { start: usize },
    DoubleQuote { start: usize },
    DollarSingleQuote { start: usize },
    Parameter { start: usize, braced: bool },
    CommandSub { start: usize },
    Backtick { start: usize },
    ArithSub { start: usize },
    Comment { start: usize },
}

// ---------------------------------------------------------------------------
// ScannerState
// ---------------------------------------------------------------------------

/// Mutable state carried through the scan.
#[derive(Debug, Clone)]
struct ScannerState {
    mode_stack: Vec<ScanMode>,
    /// True when the next non-whitespace character starts a new token at
    /// the beginning of a word (used for `#` comment detection and `~`).
    word_start: bool,
    /// True when the next word is in command position (first word of a
    /// simple command, or immediately after `|`, `&&`, `||`, `;`, etc.).
    command_position: bool,
}

impl ScannerState {
    fn new() -> Self {
        Self {
            mode_stack: vec![ScanMode::Normal],
            word_start: true,
            command_position: true,
        }
    }

    fn current_mode(&self) -> &ScanMode {
        self.mode_stack.last().unwrap_or(&ScanMode::Normal)
    }

    fn push_mode(&mut self, mode: ScanMode) {
        self.mode_stack.push(mode);
    }

    fn pop_mode(&mut self) {
        if self.mode_stack.len() > 1 {
            self.mode_stack.pop();
        }
    }
}

// ---------------------------------------------------------------------------
// Keyword tables
// ---------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "do", "done", "while", "until", "case", "esac",
    "in", "!", "{", "}",
];

/// Keywords after which the *next* word is also in command position.
const COMMAND_POSITION_KEYWORDS: &[&str] = &["then", "else", "elif", "do", "!", "time"];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

// ---------------------------------------------------------------------------
// Character classification helpers
// ---------------------------------------------------------------------------

fn is_operator_char(ch: char) -> bool {
    matches!(ch, '|' | '&' | ';')
}

fn is_redirect_start(ch: char) -> bool {
    matches!(ch, '<' | '>')
}

fn is_valid_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True for characters that cannot appear inside an unquoted word.
fn is_word_break(ch: char) -> bool {
    ch.is_ascii_whitespace()
        || is_operator_char(ch)
        || is_redirect_start(ch)
        || matches!(ch, '(' | ')' | '\'' | '"' | '`' | '$' | '#')
}

// ---------------------------------------------------------------------------
// HighlightCache
// ---------------------------------------------------------------------------

/// Cache for incremental rescanning.
struct HighlightCache {
    prev_input: Vec<char>,
    prev_spans: Vec<ColorSpan>,
    checkpoints: Vec<(usize, ScannerState)>,
    checkpoint_interval: usize,
}

impl HighlightCache {
    fn new() -> Self {
        Self {
            prev_input: Vec::new(),
            prev_spans: Vec::new(),
            checkpoints: Vec::new(),
            checkpoint_interval: 32,
        }
    }

    /// Find the first position where `input` differs from the cached input.
    fn diff_pos(&self, input: &[char]) -> usize {
        self.prev_input
            .iter()
            .zip(input.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(self.prev_input.len().min(input.len()))
    }

    /// Return the nearest checkpoint at or before `pos`.
    fn nearest_checkpoint(&self, pos: usize) -> Option<(usize, ScannerState)> {
        self.checkpoints
            .iter()
            .rev()
            .find(|(cp, _)| *cp <= pos)
            .cloned()
    }

    fn clear(&mut self) {
        self.prev_input.clear();
        self.prev_spans.clear();
        self.checkpoints.clear();
    }
}

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
            Self::mark_unclosed_errors(&state, current.len(), &mut spans);
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
            if pos > 0 && pos % self.cache.checkpoint_interval == 0 {
                if !self.cache.checkpoints.iter().any(|(cp, _)| *cp == pos) {
                    self.cache.checkpoints.push((pos, state.clone()));
                }
            }

            match state.current_mode().clone() {
                ScanMode::Normal => {
                    pos = self.scan_normal(chars, pos, state, &mut spans, checker_env);
                }
                ScanMode::SingleQuote { start } => {
                    pos = Self::scan_single_quote(chars, pos, start, state, &mut spans);
                }
                ScanMode::DoubleQuote { start } => {
                    pos = self.scan_double_quote(chars, pos, start, state, &mut spans, checker_env);
                }
                ScanMode::DollarSingleQuote { start } => {
                    pos = Self::scan_dollar_single_quote(chars, pos, start, state, &mut spans);
                }
                ScanMode::Parameter { start, braced } => {
                    pos = Self::scan_parameter(chars, pos, start, braced, state, &mut spans);
                }
                ScanMode::CommandSub { .. } => {
                    // CommandSub itself doesn't scan — it pushes Normal which does the
                    // real scanning. When Normal pops, we detect the CommandSub below
                    // and pop it too. This is already handled in scan_normal (`)` case).
                    // If we somehow end up here, just pop.
                    state.pop_mode();
                }
                ScanMode::Backtick { .. } => {
                    // Similar to CommandSub — handled by scan_normal.
                    state.pop_mode();
                }
                ScanMode::ArithSub { start } => {
                    pos = Self::scan_arith_sub(chars, pos, start, state, &mut spans);
                }
                ScanMode::Comment { start } => {
                    pos = Self::scan_comment(chars, pos, start, state, &mut spans);
                }
            }
        }

        spans
    }

    // -----------------------------------------------------------------------
    // scan_normal
    // -----------------------------------------------------------------------

    fn scan_normal(
        &mut self,
        chars: &[char],
        pos: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
        checker_env: &CheckerEnv,
    ) -> usize {
        if pos >= chars.len() {
            return pos;
        }

        let ch = chars[pos];

        // --- Whitespace ---
        if ch.is_ascii_whitespace() {
            state.word_start = true;
            return pos + 1;
        }

        // --- Comment ---
        if ch == '#' && state.word_start {
            state.push_mode(ScanMode::Comment { start: pos });
            return pos;
        }

        // --- Operators: | & ; ---
        if is_operator_char(ch) {
            let start = pos;
            let mut end = pos + 1;

            if ch == '|' && end < chars.len() && chars[end] == '|' {
                end += 1; // ||
            } else if ch == '&' && end < chars.len() && chars[end] == '&' {
                end += 1; // &&
            } else if ch == ';' && end < chars.len() && chars[end] == ';' {
                end += 1; // ;;
            }

            spans.push(ColorSpan {
                start,
                end,
                style: HighlightStyle::Operator,
            });
            state.command_position = true;
            state.word_start = true;
            return end;
        }

        // --- Redirects: < > ---
        if is_redirect_start(ch) {
            let start = pos;
            let mut end = pos + 1;

            if ch == '>' && end < chars.len() {
                match chars[end] {
                    '>' | '|' | '&' => end += 1,
                    _ => {}
                }
            } else if ch == '<' && end < chars.len() {
                match chars[end] {
                    '<' | '&' | '>' => end += 1,
                    _ => {}
                }
                // <<- (here-doc strip)
                if end == start + 2
                    && chars[start + 1] == '<'
                    && end < chars.len()
                    && chars[end] == '-'
                {
                    end += 1;
                }
            }

            spans.push(ColorSpan {
                start,
                end,
                style: HighlightStyle::Redirect,
            });
            // After a redirect the next token is a filename, not a command
            state.command_position = false;
            state.word_start = true;
            return end;
        }

        // --- Parentheses ---
        if ch == '(' {
            spans.push(ColorSpan {
                start: pos,
                end: pos + 1,
                style: HighlightStyle::Operator,
            });
            state.command_position = true;
            state.word_start = true;
            return pos + 1;
        }

        if ch == ')' {
            // Check if we are closing a CommandSub: the stack would be
            // [..., CommandSub, Normal] and current mode is Normal.
            let stack_len = state.mode_stack.len();
            if stack_len >= 2 {
                if let ScanMode::CommandSub { start } = state.mode_stack[stack_len - 2] {
                    // Pop Normal, then pop CommandSub
                    state.pop_mode(); // pops Normal
                    spans.push(ColorSpan {
                        start,
                        end: pos + 1,
                        style: HighlightStyle::CommandSub,
                    });
                    state.pop_mode(); // pops CommandSub
                    state.word_start = false;
                    state.command_position = false;
                    return pos + 1;
                }
            }

            // Otherwise, plain operator (subshell close, etc.)
            spans.push(ColorSpan {
                start: pos,
                end: pos + 1,
                style: HighlightStyle::Operator,
            });
            state.command_position = false;
            state.word_start = true;
            return pos + 1;
        }

        // --- Quotes ---
        if ch == '\'' {
            state.push_mode(ScanMode::SingleQuote { start: pos });
            state.word_start = false;
            state.command_position = false;
            return pos + 1; // skip opening quote, scan_single_quote takes over
        }

        if ch == '"' {
            state.push_mode(ScanMode::DoubleQuote { start: pos });
            state.word_start = false;
            state.command_position = false;
            return pos + 1;
        }

        // --- Backtick ---
        if ch == '`' {
            let stack_len = state.mode_stack.len();
            if stack_len >= 2 {
                if let ScanMode::Backtick { start } = state.mode_stack[stack_len - 2] {
                    // Closing backtick
                    state.pop_mode(); // pops Normal
                    spans.push(ColorSpan {
                        start,
                        end: pos + 1,
                        style: HighlightStyle::CommandSub,
                    });
                    state.pop_mode(); // pops Backtick
                    state.word_start = false;
                    state.command_position = false;
                    return pos + 1;
                }
            }
            // Opening backtick — push Backtick then Normal
            state.push_mode(ScanMode::Backtick { start: pos });
            state.push_mode(ScanMode::Normal);
            state.word_start = true;
            state.command_position = true;
            return pos + 1;
        }

        // --- Dollar expansions ---
        if ch == '$' {
            return self.scan_dollar(chars, pos, state, spans, checker_env);
        }

        // --- Tilde ---
        if ch == '~' && state.word_start {
            spans.push(ColorSpan {
                start: pos,
                end: pos + 1,
                style: HighlightStyle::Tilde,
            });
            state.word_start = false;
            // Tilde doesn't change command_position by itself — it's part of a word.
            return pos + 1;
        }

        // --- Regular word ---
        self.scan_word(chars, pos, state, spans, checker_env)
    }

    // -----------------------------------------------------------------------
    // scan_dollar – handle $... in Normal mode
    // -----------------------------------------------------------------------

    fn scan_dollar(
        &mut self,
        chars: &[char],
        pos: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
        _checker_env: &CheckerEnv,
    ) -> usize {
        let next = if pos + 1 < chars.len() {
            Some(chars[pos + 1])
        } else {
            None
        };

        match next {
            Some('\'') => {
                // $'...' — ANSI-C quoting
                state.push_mode(ScanMode::DollarSingleQuote { start: pos });
                state.word_start = false;
                state.command_position = false;
                pos + 2 // skip $'
            }
            Some('(') => {
                // Check for $(( — arithmetic
                if pos + 2 < chars.len() && chars[pos + 2] == '(' {
                    state.push_mode(ScanMode::ArithSub { start: pos });
                    state.word_start = false;
                    state.command_position = false;
                    pos + 3 // skip $((
                } else {
                    // $( — command substitution
                    state.push_mode(ScanMode::CommandSub { start: pos });
                    state.push_mode(ScanMode::Normal);
                    state.word_start = true;
                    state.command_position = true;
                    pos + 2 // skip $(
                }
            }
            Some('{') => {
                state.push_mode(ScanMode::Parameter {
                    start: pos,
                    braced: true,
                });
                state.word_start = false;
                state.command_position = false;
                pos + 2 // skip ${
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                // $NAME
                let var_start = pos;
                let mut end = pos + 1;
                while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                {
                    end += 1;
                }
                spans.push(ColorSpan {
                    start: var_start,
                    end,
                    style: HighlightStyle::Variable,
                });
                state.word_start = false;
                state.command_position = false;
                end
            }
            Some(c)
                if c.is_ascii_digit() || matches!(c, '@' | '*' | '#' | '?' | '-' | '$' | '!') =>
            {
                // $0 .. $9, $@, $*, $#, $?, $-, $$, $!
                spans.push(ColorSpan {
                    start: pos,
                    end: pos + 2,
                    style: HighlightStyle::Variable,
                });
                state.word_start = false;
                state.command_position = false;
                pos + 2
            }
            _ => {
                // Bare $ at end of input or before something unexpected – treat as
                // default text.
                state.word_start = false;
                pos + 1
            }
        }
    }

    // -----------------------------------------------------------------------
    // scan_word – collect a plain word in Normal mode
    // -----------------------------------------------------------------------

    fn scan_word(
        &mut self,
        chars: &[char],
        pos: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
        checker_env: &CheckerEnv,
    ) -> usize {
        let start = pos;
        let mut end = pos;
        while end < chars.len() && !is_word_break(chars[end]) {
            end += 1;
        }

        if end == start {
            // Safety: if nothing consumed, advance by one to avoid infinite loop.
            state.word_start = false;
            return pos + 1;
        }

        let word: String = chars[start..end].iter().collect();

        // --- Check for assignment (VAR=value) in command position ---
        if state.command_position {
            if let Some(eq_idx) = word.find('=') {
                let name_part = &word[..eq_idx];
                if !name_part.is_empty() && is_valid_name(name_part) {
                    // It's an assignment prefix. The part before = (inclusive) is
                    // Assignment; the part after is Default.
                    let eq_char_pos = start + eq_idx;
                    spans.push(ColorSpan {
                        start,
                        end: eq_char_pos + 1,
                        style: HighlightStyle::Assignment,
                    });
                    if eq_char_pos + 1 < end {
                        spans.push(ColorSpan {
                            start: eq_char_pos + 1,
                            end,
                            style: HighlightStyle::Default,
                        });
                    }
                    // command_position stays true after an assignment prefix
                    state.word_start = true;
                    return end;
                }
            }
        }

        // --- IO number: all digits followed by redirect ---
        if word.chars().all(|c| c.is_ascii_digit())
            && end < chars.len()
            && is_redirect_start(chars[end])
        {
            spans.push(ColorSpan {
                start,
                end,
                style: HighlightStyle::IoNumber,
            });
            state.word_start = false;
            // command_position unchanged
            return end;
        }

        // --- Command position: keyword or command check ---
        if state.command_position {
            if is_keyword(&word) {
                spans.push(ColorSpan {
                    start,
                    end,
                    style: HighlightStyle::Keyword,
                });
                // After a keyword, next word is generally in command position
                // (e.g., `if cmd`, `while cmd`). Some keywords end command
                // position (`fi`, `done`, `esac`, `}`), but those are followed
                // by operators anyway. We set command_position based on whether
                // this is a COMMAND_POSITION_KEYWORDS keyword.
                state.command_position = COMMAND_POSITION_KEYWORDS.contains(&word.as_str());
                // Keywords like "fi", "done" etc. act like statement terminators —
                // what follows is likely an operator, so command_position stays false
                // until an operator resets it. But for safety, keywords like "if",
                // "while", "for", "case", "until", "{" do put us in command position.
                if matches!(
                    word.as_str(),
                    "if" | "while" | "until" | "for" | "case" | "{" | "in"
                ) {
                    state.command_position = true;
                }
                state.word_start = true;
                return end;
            }

            // Check command existence
            let existence = self.checker.check(&word, checker_env);
            let style = match existence {
                CommandExistence::Valid => HighlightStyle::CommandValid,
                CommandExistence::Invalid => HighlightStyle::CommandInvalid,
            };
            spans.push(ColorSpan { start, end, style });
            state.command_position = false;
            state.word_start = true;
            return end;
        }

        // --- Default (argument) ---
        spans.push(ColorSpan {
            start,
            end,
            style: HighlightStyle::Default,
        });
        state.word_start = true;
        state.command_position = false;
        end
    }

    // -----------------------------------------------------------------------
    // scan_single_quote
    // -----------------------------------------------------------------------

    fn scan_single_quote(
        chars: &[char],
        pos: usize,
        start: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        let mut p = pos;
        while p < chars.len() {
            if chars[p] == '\'' {
                spans.push(ColorSpan {
                    start,
                    end: p + 1,
                    style: HighlightStyle::String,
                });
                state.pop_mode();
                return p + 1;
            }
            p += 1;
        }
        // Unclosed — mark_unclosed_errors will handle it
        p
    }

    // -----------------------------------------------------------------------
    // scan_double_quote
    // -----------------------------------------------------------------------

    fn scan_double_quote(
        &mut self,
        chars: &[char],
        pos: usize,
        start: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
        _checker_env: &CheckerEnv,
    ) -> usize {
        let mut p = pos;
        let mut text_start = start; // includes the opening "

        while p < chars.len() {
            match chars[p] {
                '"' => {
                    // Closing double quote
                    spans.push(ColorSpan {
                        start: text_start,
                        end: p + 1,
                        style: HighlightStyle::DoubleString,
                    });
                    state.pop_mode();
                    return p + 1;
                }
                '\\' => {
                    // Escape: skip next char
                    p += 1;
                    if p < chars.len() {
                        p += 1;
                    }
                }
                '$' => {
                    // Emit DoubleString for text accumulated so far
                    if p > text_start {
                        spans.push(ColorSpan {
                            start: text_start,
                            end: p,
                            style: HighlightStyle::DoubleString,
                        });
                    }
                    // Handle $ expansion inside double quotes
                    let next = if p + 1 < chars.len() {
                        Some(chars[p + 1])
                    } else {
                        None
                    };
                    match next {
                        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                            let var_start = p;
                            let mut end = p + 1;
                            while end < chars.len()
                                && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                            {
                                end += 1;
                            }
                            spans.push(ColorSpan {
                                start: var_start,
                                end,
                                style: HighlightStyle::Variable,
                            });
                            p = end;
                            text_start = p;
                        }
                        Some(c)
                            if c.is_ascii_digit()
                                || matches!(c, '@' | '*' | '#' | '?' | '-' | '$' | '!') =>
                        {
                            spans.push(ColorSpan {
                                start: p,
                                end: p + 2,
                                style: HighlightStyle::Variable,
                            });
                            p += 2;
                            text_start = p;
                        }
                        Some('{') => {
                            // ${...} inside double quote — scan to closing }
                            let brace_start = p;
                            p += 2; // skip ${
                            while p < chars.len() && chars[p] != '}' {
                                p += 1;
                            }
                            if p < chars.len() {
                                p += 1; // skip }
                            }
                            spans.push(ColorSpan {
                                start: brace_start,
                                end: p,
                                style: HighlightStyle::Variable,
                            });
                            text_start = p;
                        }
                        Some('(') => {
                            // $( or $(( inside double quotes
                            if p + 2 < chars.len() && chars[p + 2] == '(' {
                                // $(( — arithmetic
                                let arith_start = p;
                                p += 3;
                                while p + 1 < chars.len()
                                    && !(chars[p] == ')' && chars[p + 1] == ')')
                                {
                                    p += 1;
                                }
                                if p + 1 < chars.len() {
                                    p += 2;
                                }
                                spans.push(ColorSpan {
                                    start: arith_start,
                                    end: p,
                                    style: HighlightStyle::ArithSub,
                                });
                                text_start = p;
                            } else {
                                // $( — command sub inside double quotes
                                let cmd_start = p;
                                p += 2;
                                let mut depth = 1;
                                while p < chars.len() && depth > 0 {
                                    if chars[p] == '(' {
                                        depth += 1;
                                    } else if chars[p] == ')' {
                                        depth -= 1;
                                    }
                                    if depth > 0 {
                                        p += 1;
                                    }
                                }
                                if p < chars.len() {
                                    p += 1;
                                }
                                spans.push(ColorSpan {
                                    start: cmd_start,
                                    end: p,
                                    style: HighlightStyle::CommandSub,
                                });
                                text_start = p;
                            }
                        }
                        _ => {
                            // Bare $
                            p += 1;
                            text_start = p - 1; // include $ in next string span
                        }
                    }
                }
                '`' => {
                    // Backtick inside double quotes
                    if p > text_start {
                        spans.push(ColorSpan {
                            start: text_start,
                            end: p,
                            style: HighlightStyle::DoubleString,
                        });
                    }
                    let bt_start = p;
                    p += 1;
                    while p < chars.len() && chars[p] != '`' {
                        if chars[p] == '\\' {
                            p += 1;
                        }
                        p += 1;
                    }
                    if p < chars.len() {
                        p += 1; // skip closing `
                    }
                    spans.push(ColorSpan {
                        start: bt_start,
                        end: p,
                        style: HighlightStyle::CommandSub,
                    });
                    text_start = p;
                }
                _ => {
                    p += 1;
                }
            }
        }
        // Unclosed — mark_unclosed_errors will handle it
        p
    }

    // -----------------------------------------------------------------------
    // scan_dollar_single_quote
    // -----------------------------------------------------------------------

    fn scan_dollar_single_quote(
        chars: &[char],
        pos: usize,
        start: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        let mut p = pos;
        while p < chars.len() {
            if chars[p] == '\\' {
                // escape: skip next
                p += 1;
                if p < chars.len() {
                    p += 1;
                }
                continue;
            }
            if chars[p] == '\'' {
                spans.push(ColorSpan {
                    start,
                    end: p + 1,
                    style: HighlightStyle::String,
                });
                state.pop_mode();
                return p + 1;
            }
            p += 1;
        }
        // Unclosed
        p
    }

    // -----------------------------------------------------------------------
    // scan_parameter (braced)
    // -----------------------------------------------------------------------

    fn scan_parameter(
        chars: &[char],
        pos: usize,
        start: usize,
        _braced: bool,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        let mut p = pos;
        while p < chars.len() {
            if chars[p] == '}' {
                spans.push(ColorSpan {
                    start,
                    end: p + 1,
                    style: HighlightStyle::Variable,
                });
                state.pop_mode();
                return p + 1;
            }
            p += 1;
        }
        // Unclosed
        p
    }

    // -----------------------------------------------------------------------
    // scan_arith_sub
    // -----------------------------------------------------------------------

    fn scan_arith_sub(
        chars: &[char],
        pos: usize,
        start: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        let mut p = pos;
        while p + 1 < chars.len() {
            if chars[p] == ')' && chars[p + 1] == ')' {
                spans.push(ColorSpan {
                    start,
                    end: p + 2,
                    style: HighlightStyle::ArithSub,
                });
                state.pop_mode();
                return p + 2;
            }
            p += 1;
        }
        // Advance to end if unclosed
        chars.len()
    }

    // -----------------------------------------------------------------------
    // scan_comment
    // -----------------------------------------------------------------------

    fn scan_comment(
        chars: &[char],
        _pos: usize,
        start: usize,
        state: &mut ScannerState,
        spans: &mut Vec<ColorSpan>,
    ) -> usize {
        // Comment spans to the end of the input.
        spans.push(ColorSpan {
            start,
            end: chars.len(),
            style: HighlightStyle::Comment,
        });
        state.pop_mode();
        chars.len()
    }

    // -----------------------------------------------------------------------
    // mark_unclosed_errors
    // -----------------------------------------------------------------------

    fn mark_unclosed_errors(state: &ScannerState, input_len: usize, spans: &mut Vec<ColorSpan>) {
        for mode in &state.mode_stack {
            match mode {
                ScanMode::SingleQuote { start }
                | ScanMode::DoubleQuote { start }
                | ScanMode::DollarSingleQuote { start }
                | ScanMode::Backtick { start } => {
                    spans.push(ColorSpan {
                        start: *start,
                        end: input_len,
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::CommandSub { start } | ScanMode::Parameter { start, .. } => {
                    // Error on opening delimiter only (2 chars for $( or ${)
                    spans.push(ColorSpan {
                        start: *start,
                        end: (*start + 2).min(input_len),
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::ArithSub { start } => {
                    // Error on opening delimiter only (3 chars for $((  )
                    spans.push(ColorSpan {
                        start: *start,
                        end: (*start + 3).min(input_len),
                        style: HighlightStyle::Error,
                    });
                }
                ScanMode::Normal | ScanMode::Comment { .. } => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::aliases::AliasStore;

    fn make_aliases() -> AliasStore {
        AliasStore::default()
    }

    // Helpers -----------------------------------------------------------------

    fn checker_env<'a>(path: &'a str, aliases: &'a AliasStore) -> CheckerEnv<'a> {
        CheckerEnv { path, aliases }
    }

    // Tests -------------------------------------------------------------------

    #[test]
    fn test_checker_builtin_special() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let env = checker_env("", &aliases);

        // Special builtins
        assert_eq!(checker.check("export", &env), CommandExistence::Valid);
        assert_eq!(checker.check("cd", &env), CommandExistence::Valid);
        // Regular builtins
        assert_eq!(checker.check("echo", &env), CommandExistence::Valid);
        assert_eq!(checker.check("true", &env), CommandExistence::Valid);
    }

    #[test]
    fn test_checker_alias() {
        let mut checker = CommandChecker::new();
        let mut aliases = make_aliases();
        aliases.set("ll", "ls -l");

        let env = checker_env("", &aliases);
        assert_eq!(checker.check("ll", &env), CommandExistence::Valid);
        assert_eq!(checker.check("zz", &env), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_path_search() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let path = "/usr/bin:/bin";
        let env = checker_env(path, &aliases);

        assert_eq!(checker.check("ls", &env), CommandExistence::Valid);
        assert_eq!(
            checker.check("xyzzy_nonexistent", &env),
            CommandExistence::Invalid
        );
    }

    #[test]
    fn test_checker_path_cache_invalidation() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();

        // First check with real PATH — ls should be found.
        let env1 = checker_env("/usr/bin:/bin", &aliases);
        assert_eq!(checker.check("ls", &env1), CommandExistence::Valid);

        // Now check with empty PATH — cache must be invalidated.
        let env2 = checker_env("", &aliases);
        assert_eq!(checker.check("ls", &env2), CommandExistence::Invalid);
    }

    #[test]
    fn test_checker_direct_path() {
        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let env = checker_env("", &aliases);

        assert_eq!(checker.check("/bin/sh", &env), CommandExistence::Valid);
        assert_eq!(
            checker.check("./nonexistent_script_xyz", &env),
            CommandExistence::Invalid
        );
    }

    #[test]
    fn test_checker_path_with_tempfile() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("create tempdir");
        let bin_path = dir.path().join("my_test_cmd");

        // Write a minimal shell script and make it executable.
        fs::write(&bin_path, "#!/bin/sh\n").expect("write temp executable");
        let mut perms = fs::metadata(&bin_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin_path, perms).expect("set permissions");

        let mut checker = CommandChecker::new();
        let aliases = make_aliases();
        let path_val = dir.path().to_str().unwrap().to_string();
        let env = checker_env(&path_val, &aliases);

        assert_eq!(checker.check("my_test_cmd", &env), CommandExistence::Valid);
        assert_eq!(
            checker.check("nosuchthing", &env),
            CommandExistence::Invalid
        );
    }

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
