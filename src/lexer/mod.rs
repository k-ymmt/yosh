pub mod token;

use std::collections::{HashMap, HashSet};

use crate::error::{self, ShellError, ShellErrorKind};
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};
use token::{Span, SpannedToken, Token};

pub struct LexerState {
    pub pos: usize,
    pub line: usize,
    pub column: usize,
    alias_token_queue: Vec<SpannedToken>,
    check_alias: bool,
    expanding_aliases: HashSet<String>,
}

pub struct PendingHereDoc {
    pub delimiter: String,
    #[allow(dead_code)]
    pub quoted: bool,
    pub strip_tabs: bool,
}

pub struct Lexer {
    input: Vec<u8>,
    pos: usize,
    line: usize,
    column: usize,
    pub pending_heredocs: Vec<PendingHereDoc>,
    heredoc_bodies: Vec<Vec<WordPart>>,
    aliases: HashMap<String, String>,
    expanding_aliases: HashSet<String>,
    check_alias: bool,
    /// Queue of tokens produced by alias expansion, to be returned before reading more input.
    alias_token_queue: Vec<SpannedToken>,
}

fn is_name_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_name_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            column: 1,
            pending_heredocs: Vec::new(),
            heredoc_bodies: Vec::new(),
            aliases: HashMap::new(),
            expanding_aliases: HashSet::new(),
            check_alias: true,
            alias_token_queue: Vec::new(),
        }
    }

    pub fn new_with_aliases(input: &str, aliases: &crate::env::aliases::AliasStore) -> Self {
        let mut lexer = Self::new(input);
        for (name, value) in aliases.sorted_iter() {
            lexer.aliases.insert(name.to_string(), value.to_string());
        }
        lexer
    }

    /// Returns the current byte position in the input.
    /// Since alias expansion uses a token queue instead of rewriting the buffer,
    /// this position always maps to the original input.
    pub fn position(&self) -> usize {
        self.pos
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn current_byte(&self) -> u8 {
        if self.at_end() {
            0
        } else {
            self.input[self.pos]
        }
    }

    fn peek_byte(&self) -> u8 {
        if self.pos + 1 >= self.input.len() {
            0
        } else {
            self.input[self.pos + 1]
        }
    }

    fn advance(&mut self) -> u8 {
        let ch = self.current_byte();
        if !self.at_end() {
            if ch == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
        ch
    }

    fn current_span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    pub fn save_state(&self) -> LexerState {
        LexerState {
            pos: self.pos,
            line: self.line,
            column: self.column,
            alias_token_queue: self.alias_token_queue.clone(),
            check_alias: self.check_alias,
            expanding_aliases: self.expanding_aliases.clone(),
        }
    }

    pub fn restore_state(&mut self, state: LexerState) {
        self.pos = state.pos;
        self.line = state.line;
        self.column = state.column;
        self.alias_token_queue = state.alias_token_queue;
        self.check_alias = state.check_alias;
        self.expanding_aliases = state.expanding_aliases;
    }

    pub fn register_heredoc(&mut self, delimiter: String, quoted: bool, strip_tabs: bool) {
        self.pending_heredocs.push(PendingHereDoc { delimiter, quoted, strip_tabs });
    }

    pub fn take_heredoc_body(&mut self) -> Option<Vec<WordPart>> {
        if self.heredoc_bodies.is_empty() {
            None
        } else {
            Some(self.heredoc_bodies.remove(0))
        }
    }

    pub fn process_pending_heredocs(&mut self) -> error::Result<()> {
        let pending: Vec<PendingHereDoc> = self.pending_heredocs.drain(..).collect();
        for hd in pending {
            let body = self.read_heredoc_body(&hd)?;
            self.heredoc_bodies.push(body);
        }
        Ok(())
    }

    fn read_heredoc_body(&mut self, hd: &PendingHereDoc) -> error::Result<Vec<WordPart>> {
        let mut body = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::InvalidHereDoc,
                    self.line,
                    self.column,
                    format!("here-document delimited by '{}' was not closed", hd.delimiter),
                ));
            }
            // Read a line
            let mut line = String::new();
            while !self.at_end() && self.current_byte() != b'\n' {
                line.push(self.current_byte() as char);
                self.advance();
            }
            if !self.at_end() {
                self.advance(); // consume newline
            }

            // Strip leading tabs if <<-
            let check_line = if hd.strip_tabs {
                line.trim_start_matches('\t').to_string()
            } else {
                line.clone()
            };

            // Check if this is the delimiter line
            if check_line == hd.delimiter {
                break;
            }

            // Add line to body (with tab stripping if applicable)
            if hd.strip_tabs {
                body.push_str(line.trim_start_matches('\t'));
            } else {
                body.push_str(&line);
            }
            body.push('\n');
        }
        Ok(vec![WordPart::Literal(body)])
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.current_byte() {
                b' ' | b'\t' => {
                    self.advance();
                }
                b'#' => {
                    // skip until newline (but don't consume the newline)
                    while !self.at_end() && self.current_byte() != b'\n' {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> error::Result<SpannedToken> {
        // Return queued tokens from alias expansion first
        if let Some(tok) = self.alias_token_queue.first().cloned() {
            self.alias_token_queue.remove(0);
            // Update check_alias based on this queued token
            self.update_check_alias_after(&tok.token);
            return Ok(tok);
        }

        let tok = self.next_token_raw()?;

        // Check for alias expansion on Word tokens when check_alias is set
        if self.check_alias
            && let Token::Word(ref word) = tok.token
            && let Some(word_text) = word.as_literal()
            && !self.expanding_aliases.contains(word_text)
            && let Some(alias_value) = self.aliases.get(word_text).cloned()
        {
            let word_text = word_text.to_string();
            // Mark as expanding to prevent recursion
            self.expanding_aliases.insert(word_text);

            // Check if alias value ends with whitespace —
            // if so, the next word after the expansion should also be alias-checked
            let trailing_space = alias_value.ends_with(' ')
                || alias_value.ends_with('\t');

            // Tokenize the alias value into a separate token stream
            let mut alias_lexer = Lexer::new(&alias_value);
            // Copy aliases and expanding_aliases to the sub-lexer
            alias_lexer.aliases = self.aliases.clone();
            alias_lexer.expanding_aliases = self.expanding_aliases.clone();
            alias_lexer.check_alias = true;

            let mut tokens = Vec::new();
            loop {
                let t = alias_lexer.next_token()?;
                if t.token == Token::Eof {
                    break;
                }
                tokens.push(t);
            }

            // Merge back any recursion-prevention state
            self.expanding_aliases = alias_lexer.expanding_aliases;

            if tokens.is_empty() {
                // Alias expanded to nothing, get next token
                if trailing_space {
                    self.check_alias = true;
                }
                return self.next_token();
            }

            // Return the first token, queue the rest
            let first = tokens.remove(0);
            self.alias_token_queue = tokens;
            self.update_check_alias_after(&first.token);

            // If alias value ends with space/tab, force next
            // word to be alias-checked (overrides normal behavior)
            if trailing_space {
                self.check_alias = true;
            }

            return Ok(first);
        }

        // After producing a token, decide if next word should be alias-checked
        self.update_check_alias_after(&tok.token);

        Ok(tok)
    }

    fn update_check_alias_after(&mut self, token: &Token) {
        match token {
            Token::Semi | Token::Newline | Token::Pipe | Token::AndIf | Token::OrIf
            | Token::LParen | Token::Amp | Token::DSemi => {
                self.check_alias = true;
            }
            Token::Word(_) => {
                // After first word in command position, stop alias-expanding
                // (unless a previous alias ended with space, already handled above)
                if self.check_alias {
                    self.check_alias = false;
                }
            }
            _ => {
                // For other tokens (redirects, etc.), don't change check_alias
            }
        }
    }

    fn next_token_raw(&mut self) -> error::Result<SpannedToken> {
        self.skip_whitespace_and_comments();

        if self.at_end() {
            let span = self.current_span();
            return Ok(SpannedToken {
                token: Token::Eof,
                span,
            });
        }

        let span = self.current_span();
        match self.current_byte() {
            b'\n' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::Newline,
                    span,
                })
            }
            b'|' => self.read_pipe(),
            b'&' => self.read_amp(),
            b';' => self.read_semi(),
            b'(' => {
                let span = self.current_span();
                self.advance();
                Ok(SpannedToken {
                    token: Token::LParen,
                    span,
                })
            }
            b')' => {
                let span = self.current_span();
                self.advance();
                Ok(SpannedToken {
                    token: Token::RParen,
                    span,
                })
            }
            b'<' => self.read_less(),
            b'>' => self.read_great(),
            ch => {
                if ch.is_ascii_digit() && let Some(io_num) = self.try_read_io_number() {
                    return Ok(SpannedToken { token: io_num, span });
                }
                self.read_word()
            }
        }
    }

    fn read_pipe(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '|'
        if self.current_byte() == b'|' {
            self.advance();
            Ok(SpannedToken {
                token: Token::OrIf,
                span,
            })
        } else {
            Ok(SpannedToken {
                token: Token::Pipe,
                span,
            })
        }
    }

    fn read_amp(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '&'
        if self.current_byte() == b'&' {
            self.advance();
            Ok(SpannedToken {
                token: Token::AndIf,
                span,
            })
        } else {
            Ok(SpannedToken {
                token: Token::Amp,
                span,
            })
        }
    }

    fn read_semi(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume ';'
        match self.current_byte() {
            b';' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::DSemi,
                    span,
                })
            }
            b'&' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::SemiAnd,
                    span,
                })
            }
            _ => Ok(SpannedToken {
                token: Token::Semi,
                span,
            }),
        }
    }

    fn read_less(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '<'
        match self.current_byte() {
            b'<' => {
                self.advance();
                if self.current_byte() == b'-' {
                    self.advance();
                    Ok(SpannedToken {
                        token: Token::DLessDash,
                        span,
                    })
                } else {
                    Ok(SpannedToken {
                        token: Token::DLess,
                        span,
                    })
                }
            }
            b'&' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::LessAnd,
                    span,
                })
            }
            b'>' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::LessGreat,
                    span,
                })
            }
            _ => Ok(SpannedToken {
                token: Token::Less,
                span,
            }),
        }
    }

    fn read_great(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        self.advance(); // consume '>'
        match self.current_byte() {
            b'>' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::DGreat,
                    span,
                })
            }
            b'&' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::GreatAnd,
                    span,
                })
            }
            b'|' => {
                self.advance();
                Ok(SpannedToken {
                    token: Token::Clobber,
                    span,
                })
            }
            _ => Ok(SpannedToken {
                token: Token::Great,
                span,
            }),
        }
    }

    fn is_meta_or_whitespace(ch: u8) -> bool {
        matches!(
            ch,
            b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | b' ' | b'\t' | b'\n'
        )
    }

    // ---- Task 5: word scanning with quoting ----

    /// Main word-part reader that loops over input, dispatching to quoting methods.
    /// `in_double_quote`: true when inside "..."
    /// `end_byte`: if Some(b'}'), stop at unquoted `}` (for ${...} parsing)
    pub fn read_word_parts(
        &mut self,
        in_double_quote: bool,
        end_byte: Option<u8>,
    ) -> error::Result<Vec<WordPart>> {
        let mut parts: Vec<WordPart> = Vec::new();
        let mut literal = String::new();

        loop {
            if self.at_end() {
                break;
            }

            let ch = self.current_byte();

            // check end_byte (e.g. '}' for ${...})
            if let Some(end) = end_byte && ch == end {
                break;
            }

            if in_double_quote {
                // inside double quotes: stop at closing "
                if ch == b'"' {
                    break;
                }
                match ch {
                    b'$' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_dollar()?;
                        parts.push(part);
                    }
                    b'\\' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backslash_in_double_quote()?;
                        parts.push(part);
                    }
                    b'`' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backtick()?;
                        parts.push(part);
                    }
                    _ => {
                        literal.push(ch as char);
                        self.advance();
                    }
                }
            } else {
                // unquoted context
                // Inside ${...} (end_byte == '}'), spaces are literal, not delimiters
                if end_byte == Some(b'}') {
                    if ch == b'}' { break; }
                } else if Self::is_meta_or_whitespace(ch) {
                    break;
                }
                match ch {
                    b'\'' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_single_quote()?;
                        parts.push(part);
                    }
                    b'"' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_double_quote()?;
                        parts.push(part);
                    }
                    b'\\' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backslash()?;
                        parts.push(part);
                    }
                    b'$' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_dollar()?;
                        parts.push(part);
                    }
                    b'`' => {
                        if !literal.is_empty() {
                            parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                        }
                        let part = self.read_backtick()?;
                        parts.push(part);
                    }
                    b'~' if parts.is_empty() && literal.is_empty() => {
                        let part = self.read_tilde();
                        parts.push(part);
                    }
                    _ => {
                        literal.push(ch as char);
                        self.advance();
                    }
                }
            }
        }

        if !literal.is_empty() {
            parts.push(WordPart::Literal(literal));
        }

        // filter out empty Literal("") parts (from line continuations)
        let parts = parts
            .into_iter()
            .filter(|p| !matches!(p, WordPart::Literal(s) if s.is_empty()))
            .collect();

        Ok(parts)
    }

    fn read_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut content = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedSingleQuote,
                    span.line,
                    span.column,
                    "unterminated single quote",
                ));
            }
            let ch = self.current_byte();
            if ch == b'\'' {
                self.advance(); // consume closing '
                break;
            }
            content.push(ch as char);
            self.advance();
        }
        Ok(WordPart::SingleQuoted(content))
    }

    fn read_double_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening "
        let inner = self.read_word_parts(true, None)?;
        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedDoubleQuote,
                span.line,
                span.column,
                "unterminated double quote",
            ));
        }
        self.advance(); // consume closing "
        Ok(WordPart::DoubleQuoted(inner))
    }

    /// Handles `\` outside double quotes.
    /// `\<newline>` is line continuation (returns empty Literal, filtered later).
    /// Otherwise returns literal of next char.
    fn read_backslash(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        if ch == b'\n' {
            // line continuation: consume newline, return empty literal (filtered later)
            self.advance();
            Ok(WordPart::Literal(String::new()))
        } else {
            self.advance();
            Ok(WordPart::Literal((ch as char).to_string()))
        }
    }

    /// Inside double quotes, `\` only escapes `$ ` " \ newline`.
    fn read_backslash_in_double_quote(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '\'
        if self.at_end() {
            return Ok(WordPart::Literal("\\".to_string()));
        }
        let ch = self.current_byte();
        match ch {
            b'$' | b'`' | b'"' | b'\\' => {
                self.advance();
                Ok(WordPart::Literal((ch as char).to_string()))
            }
            b'\n' => {
                // line continuation
                self.advance();
                Ok(WordPart::Literal(String::new()))
            }
            _ => {
                // backslash is kept literally
                self.advance();
                Ok(WordPart::Literal(format!("\\{}", ch as char)))
            }
        }
    }

    /// Handles `$'...'` with C-style escape sequences.
    /// Called after `$` is consumed; current byte is `'`.
    fn read_dollar_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut content = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedDollarSingleQuote,
                    span.line,
                    span.column,
                    "unterminated $'...' quote",
                ));
            }
            let ch = self.current_byte();
            if ch == b'\'' {
                self.advance(); // consume closing '
                break;
            }
            if ch == b'\\' {
                self.advance(); // consume '\'
                if self.at_end() {
                    content.push('\\');
                    break;
                }
                let esc = self.current_byte();
                self.advance();
                match esc {
                    b'a' => content.push('\x07'),
                    b'b' => content.push('\x08'),
                    b'e' | b'E' => content.push('\x1B'),
                    b'f' => content.push('\x0C'),
                    b'n' => content.push('\n'),
                    b'r' => content.push('\r'),
                    b't' => content.push('\t'),
                    b'v' => content.push('\x0B'),
                    b'\\' => content.push('\\'),
                    b'\'' => content.push('\''),
                    b'"' => content.push('"'),
                    b'x' => {
                        let val = self.read_hex_digits(2);
                        content.push(val as char);
                    }
                    b'c' => {
                        // \cX — control character
                        if !self.at_end() {
                            let ctrl = self.current_byte();
                            self.advance();
                            content.push((ctrl & 0x1f) as char);
                        }
                    }
                    b'0'..=b'7' => {
                        // octal: up to 3 digits, first digit already consumed
                        let mut val = (esc - b'0') as u32;
                        for _ in 0..2 {
                            let next = self.current_byte();
                            if (b'0'..=b'7').contains(&next) {
                                val = val * 8 + (next - b'0') as u32;
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let c = char::from_u32(val).unwrap_or('\u{FFFD}');
                        content.push(c);
                    }
                    _ => {
                        // unknown escape: keep backslash and char
                        content.push('\\');
                        content.push(esc as char);
                    }
                }
            } else {
                content.push(ch as char);
                self.advance();
            }
        }
        Ok(WordPart::DollarSingleQuoted(content))
    }

    /// Helper for \xHH — reads up to `max` hex digits and returns the byte value.
    fn read_hex_digits(&mut self, max: usize) -> u8 {
        let mut val: u8 = 0;
        for _ in 0..max {
            let ch = self.current_byte();
            let digit = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => break,
            };
            val = val.wrapping_mul(16).wrapping_add(digit);
            self.advance();
        }
        val
    }

    /// Reads `~` or `~user` at word start.
    fn read_tilde(&mut self) -> WordPart {
        self.advance(); // consume '~'
        let mut username = String::new();
        // read until metachar, whitespace, or '/'
        while !self.at_end() {
            let ch = self.current_byte();
            if Self::is_meta_or_whitespace(ch) || ch == b'/' {
                break;
            }
            username.push(ch as char);
            self.advance();
        }
        if username.is_empty() {
            WordPart::Tilde(None)
        } else {
            WordPart::Tilde(Some(username))
        }
    }

    // ---- Task 6: dollar expansions and backtick ----

    /// Reads [a-zA-Z_][a-zA-Z0-9_]* from current position.
    fn read_name(&mut self) -> String {
        let mut name = String::new();
        while !self.at_end() && is_name_char(self.current_byte()) {
            name.push(self.current_byte() as char);
            self.advance();
        }
        name
    }

    /// Converts a name string to ParamExpr.
    fn classify_param_name(&self, name: &str) -> ParamExpr {
        match name {
            "@" => ParamExpr::Special(SpecialParam::At),
            "*" => ParamExpr::Special(SpecialParam::Star),
            "#" => ParamExpr::Special(SpecialParam::Hash),
            "?" => ParamExpr::Special(SpecialParam::Question),
            "-" => ParamExpr::Special(SpecialParam::Dash),
            "$" => ParamExpr::Special(SpecialParam::Dollar),
            "!" => ParamExpr::Special(SpecialParam::Bang),
            "0" => ParamExpr::Special(SpecialParam::Zero),
            s if s.chars().all(|c| c.is_ascii_digit()) => {
                let n: usize = s.parse().unwrap_or(0);
                ParamExpr::Positional(n)
            }
            s => ParamExpr::Simple(s.to_string()),
        }
    }

    /// Reads the parameter name inside braces (after `${`).
    fn read_param_name(&mut self, span: Span) -> error::Result<String> {
        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                "unterminated parameter expansion",
            ));
        }
        let ch = self.current_byte();
        match ch {
            b'@' | b'*' | b'?' | b'-' | b'$' | b'!' | b'0' | b'#' => {
                self.advance();
                Ok((ch as char).to_string())
            }
            b'1'..=b'9' => {
                let mut digits = String::new();
                while !self.at_end() && self.current_byte().is_ascii_digit() {
                    digits.push(self.current_byte() as char);
                    self.advance();
                }
                Ok(digits)
            }
            c if is_name_start(c) => {
                Ok(self.read_name())
            }
            _ => Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("invalid parameter name character: '{}'", ch as char),
            )),
        }
    }

    /// Reads word parts until `}`, then consumes `}`.
    fn read_word_in_brace(&mut self, span: Span) -> error::Result<Word> {
        let parts = self.read_word_parts(false, Some(b'}'))?;
        self.expect_byte(b'}', span)?;
        Ok(Word { parts })
    }

    /// Expects the given byte at current position, consuming it on success.
    fn expect_byte(&mut self, expected: u8, span: Span) -> error::Result<()> {
        if self.at_end() || self.current_byte() != expected {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("expected '{}'", expected as char),
            ));
        }
        self.advance();
        Ok(())
    }

    /// Handles `${...}` braced parameter expansion.
    fn read_param_expansion_braced(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume '{'

        // Check for ${#name} — length operator
        if !self.at_end() && self.current_byte() == b'#' {
            // peek ahead: if next char is '}' or an operator, this is ${#} (special Hash)
            // if next char starts a name or is a digit/special, it's ${#name}
            let next = self.peek_byte();
            match next {
                b'}' => {
                    // ${#} — special Hash param
                    self.advance(); // consume '#'
                    self.expect_byte(b'}', span)?;
                    return Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash)));
                }
                c if is_name_start(c) || c.is_ascii_digit() => {
                    self.advance(); // consume '#'
                    let name = self.read_param_name(span)?;
                    self.expect_byte(b'}', span)?;
                    // ${#name} where name is a positional or special — still Length
                    // But ${#@}, ${#*} etc. in POSIX are technically invalid; we return Length anyway
                    return Ok(WordPart::Parameter(ParamExpr::Length(name)));
                }
                _ => {
                    // ${#operator...} — treat '#' as the name (special Hash param), then operator
                    // Actually treat it as param name '#'
                    self.advance(); // consume '#'
                    let name = "#".to_string();
                    // fall through to operator handling below
                    return self.read_param_operator(span, name);
                }
            }
        }

        let name = self.read_param_name(span)?;
        self.read_param_operator(span, name)
    }

    /// After reading the param name inside `${`, read optional operator and closing `}`.
    fn read_param_operator(&mut self, span: Span, name: String) -> error::Result<WordPart> {
        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                "unterminated parameter expansion",
            ));
        }

        let ch = self.current_byte();
        match ch {
            b'}' => {
                self.advance(); // consume '}'
                Ok(WordPart::Parameter(self.classify_param_name(&name)))
            }
            b'%' => {
                self.advance(); // consume '%'
                if !self.at_end() && self.current_byte() == b'%' {
                    self.advance(); // consume second '%'
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripLongSuffix(name, word)))
                } else {
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripShortSuffix(name, word)))
                }
            }
            b'#' => {
                self.advance(); // consume '#'
                if !self.at_end() && self.current_byte() == b'#' {
                    self.advance(); // consume second '#'
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripLongPrefix(name, word)))
                } else {
                    let word = self.read_word_in_brace(span)?;
                    Ok(WordPart::Parameter(ParamExpr::StripShortPrefix(name, word)))
                }
            }
            b':' => {
                self.advance(); // consume ':'
                self.read_conditional_param(span, name, true)
            }
            b'-' | b'=' | b'?' | b'+' => {
                self.read_conditional_param(span, name, false)
            }
            _ => Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("unexpected character in parameter expansion: '{}'", ch as char),
            )),
        }
    }

    /// Reads the conditional operator (`-`, `=`, `?`, `+`) and its word argument.
    fn read_conditional_param(&mut self, span: Span, name: String, null_check: bool) -> error::Result<WordPart> {
        if self.at_end() {
            return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                "unterminated parameter expansion",
            ));
        }
        let op = self.current_byte();
        self.advance(); // consume operator

        // Read optional word (may be empty)
        let parts = self.read_word_parts(false, Some(b'}'))?;
        self.expect_byte(b'}', span)?;
        let word = if parts.is_empty() { None } else { Some(Word { parts }) };

        let expr = match op {
            b'-' => ParamExpr::Default { name, word, null_check },
            b'=' => ParamExpr::Assign { name, word, null_check },
            b'?' => ParamExpr::Error { name, word, null_check },
            b'+' => ParamExpr::Alt { name, word, null_check },
            _ => return Err(ShellError::new(
                ShellErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("unknown parameter operator: '{}'", op as char),
            )),
        };
        Ok(WordPart::Parameter(expr))
    }

    /// Handles `$(...)` command substitution.
    fn read_command_sub_dollar(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        let content = self.read_balanced_parens(span)?;
        let mut parser = crate::parser::Parser::new(&content);
        let program = parser.parse_program()?;
        Ok(WordPart::CommandSub(program))
    }

    /// Handles `$((expr))` arithmetic expansion.
    fn read_arith_expansion(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume first '('
        self.advance(); // consume second '('

        let mut expr = String::new();
        let mut depth: usize = 0;

        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedArithSub,
                    span.line,
                    span.column,
                    "unterminated arithmetic expansion",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'(' => {
                    depth += 1;
                    expr.push('(');
                    self.advance();
                }
                b')' => {
                    if depth == 0 {
                        // check for closing '))'
                        self.advance(); // consume first ')'
                        if self.at_end() || self.current_byte() != b')' {
                            return Err(ShellError::new(
                                ShellErrorKind::UnterminatedArithSub,
                                span.line,
                                span.column,
                                "expected '))'",
                            ));
                        }
                        self.advance(); // consume second ')'
                        break;
                    } else {
                        depth -= 1;
                        expr.push(')');
                        self.advance();
                    }
                }
                _ => {
                    expr.push(ch as char);
                    self.advance();
                }
            }
        }

        Ok(WordPart::ArithSub(expr.trim().to_string()))
    }

    /// Reads balanced parentheses content for `$(...)`.
    /// Returns content between outer parens (not including them).
    fn read_balanced_parens(&mut self, span: Span) -> error::Result<String> {
        self.advance(); // consume opening '('
        let mut content = String::new();
        let mut depth: usize = 1;

        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedCommandSub,
                    span.line,
                    span.column,
                    "unterminated command substitution",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'(' => {
                    depth += 1;
                    content.push('(');
                    self.advance();
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance(); // consume closing ')'
                        break;
                    }
                    content.push(')');
                    self.advance();
                }
                b'\'' => {
                    // single quote: read until closing '
                    content.push('\'');
                    self.advance();
                    loop {
                        if self.at_end() {
                            return Err(ShellError::new(
                                ShellErrorKind::UnterminatedCommandSub,
                                span.line,
                                span.column,
                                "unterminated single quote in command substitution",
                            ));
                        }
                        let qch = self.current_byte();
                        content.push(qch as char);
                        self.advance();
                        if qch == b'\'' {
                            break;
                        }
                    }
                }
                b'"' => {
                    // double quote: read until closing ", handling backslash escapes
                    content.push('"');
                    self.advance();
                    loop {
                        if self.at_end() {
                            return Err(ShellError::new(
                                ShellErrorKind::UnterminatedCommandSub,
                                span.line,
                                span.column,
                                "unterminated double quote in command substitution",
                            ));
                        }
                        let qch = self.current_byte();
                        if qch == b'"' {
                            content.push('"');
                            self.advance();
                            break;
                        }
                        if qch == b'\\' {
                            content.push('\\');
                            self.advance();
                            if !self.at_end() {
                                content.push(self.current_byte() as char);
                                self.advance();
                            }
                        } else {
                            content.push(qch as char);
                            self.advance();
                        }
                    }
                }
                _ => {
                    content.push(ch as char);
                    self.advance();
                }
            }
        }

        Ok(content)
    }

    /// Handles `$`. Dispatches to appropriate expansion method.
    fn read_dollar(&mut self) -> error::Result<WordPart> {
        self.advance(); // consume '$'

        if self.at_end() {
            return Ok(WordPart::Literal("$".to_string()));
        }

        let ch = self.current_byte();
        match ch {
            b'\'' => self.read_dollar_single_quote(),
            b'{' => self.read_param_expansion_braced(),
            b'(' => {
                // check for $(( arithmetic ))
                if self.peek_byte() == b'(' {
                    self.read_arith_expansion()
                } else {
                    self.read_command_sub_dollar()
                }
            }
            b'@' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::At)))
            }
            b'*' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Star)))
            }
            b'#' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Hash)))
            }
            b'?' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Question)))
            }
            b'-' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Dash)))
            }
            b'$' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Dollar)))
            }
            b'!' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Bang)))
            }
            b'0' => {
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Special(SpecialParam::Zero)))
            }
            b'1'..=b'9' => {
                let digit = ch - b'0';
                self.advance();
                Ok(WordPart::Parameter(ParamExpr::Positional(digit as usize)))
            }
            c if is_name_start(c) => {
                let name = self.read_name();
                Ok(WordPart::Parameter(ParamExpr::Simple(name)))
            }
            _ => Ok(WordPart::Literal("$".to_string())),
        }
    }

    /// Handles backtick command substitution `` `...` ``.
    fn read_backtick(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '`'
        let mut content = String::new();

        loop {
            if self.at_end() {
                return Err(ShellError::new(
                    ShellErrorKind::UnterminatedBacktick,
                    span.line,
                    span.column,
                    "unterminated backtick command substitution",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'`' => {
                    self.advance(); // consume closing '`'
                    break;
                }
                b'\\' => {
                    self.advance(); // consume '\'
                    if self.at_end() {
                        content.push('\\');
                        break;
                    }
                    let esc = self.current_byte();
                    match esc {
                        b'$' | b'`' | b'\\' => {
                            content.push(esc as char);
                            self.advance();
                        }
                        _ => {
                            // keep backslash literally
                            content.push('\\');
                            content.push(esc as char);
                            self.advance();
                        }
                    }
                }
                _ => {
                    content.push(ch as char);
                    self.advance();
                }
            }
        }

        let mut parser = crate::parser::Parser::new(&content);
        let program = parser.parse_program()?;
        Ok(WordPart::CommandSub(program))
    }

    fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let parts = self.read_word_parts(false, None)?;
        if parts.is_empty() {
            return Err(ShellError::new(
                ShellErrorKind::UnexpectedToken,
                span.line,
                span.column,
                format!("unexpected character: '{}'", self.current_byte() as char),
            ));
        }
        Ok(SpannedToken {
            token: Token::Word(Word { parts }),
            span,
        })
    }

    // ---- Task 7: IO_NUMBER detection ----

    /// Tries to read an IO_NUMBER token (digits immediately followed by `<` or `>`).
    /// Returns None and restores state if not followed by a redirect operator.
    fn try_read_io_number(&mut self) -> Option<Token> {
        let state = self.save_state();
        let mut digits = String::new();

        while !self.at_end() && self.current_byte().is_ascii_digit() {
            digits.push(self.current_byte() as char);
            self.advance();
        }

        let next = self.current_byte();
        if (next == b'<' || next == b'>') && let Ok(n) = digits.parse::<i32>() {
            return Some(Token::IoNumber(n));
        }

        // Not an IO_NUMBER: restore state
        self.restore_state(state);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{ParamExpr, SpecialParam, WordPart};

    fn tokenize(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let st = lexer.next_token().unwrap();
            if st.token == Token::Eof {
                break;
            }
            tokens.push(st.token);
        }
        tokens
    }

    // ---- Task 4 tests ----

    #[test]
    fn test_empty_input() {
        assert_eq!(tokenize(""), vec![]);
    }

    #[test]
    fn test_newline() {
        assert_eq!(tokenize("\n"), vec![Token::Newline]);
    }

    #[test]
    fn test_single_char_operators() {
        assert_eq!(
            tokenize("| ; & ( )"),
            vec![Token::Pipe, Token::Semi, Token::Amp, Token::LParen, Token::RParen]
        );
    }

    #[test]
    fn test_multi_char_operators() {
        assert_eq!(
            tokenize("&& || ;; ;&"),
            vec![Token::AndIf, Token::OrIf, Token::DSemi, Token::SemiAnd]
        );
    }

    #[test]
    fn test_redirect_operators() {
        assert_eq!(
            tokenize("< > >> <& >& <> >|"),
            vec![
                Token::Less,
                Token::Great,
                Token::DGreat,
                Token::LessAnd,
                Token::GreatAnd,
                Token::LessGreat,
                Token::Clobber
            ]
        );
    }

    #[test]
    fn test_heredoc_operators() {
        assert_eq!(tokenize("<< <<-"), vec![Token::DLess, Token::DLessDash]);
    }

    #[test]
    fn test_comment_ignored() {
        assert_eq!(tokenize("# this is a comment\n"), vec![Token::Newline]);
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(tokenize("   \t  "), vec![]);
    }

    // ---- Task 5 tests ----

    #[test]
    fn test_unquoted_words() {
        let tokens = tokenize("echo hello world");
        assert_eq!(
            tokens,
            vec![
                Token::Word(Word::literal("echo")),
                Token::Word(Word::literal("hello")),
                Token::Word(Word::literal("world")),
            ]
        );
    }

    #[test]
    fn test_single_quoted_word() {
        let tokens = tokenize("echo 'hello world'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::SingleQuoted("hello world".to_string())]
            })
        );
    }

    #[test]
    fn test_double_quoted_word() {
        let tokens = tokenize("echo \"hello world\"");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::DoubleQuoted(vec![WordPart::Literal(
                    "hello world".to_string()
                )])],
            })
        );
    }

    #[test]
    fn test_backslash_escape() {
        let tokens = tokenize("echo hello\\ world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hello".to_string()),
                    WordPart::Literal(" ".to_string()),
                    WordPart::Literal("world".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_line_continuation() {
        let tokens = tokenize("echo hel\\\nlo");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hel".to_string()),
                    WordPart::Literal("lo".to_string())
                ],
            })
        );
    }

    #[test]
    fn test_dollar_single_quote() {
        let tokens = tokenize("echo $'hello\\nworld'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::DollarSingleQuoted("hello\nworld".to_string())],
            })
        );
    }

    #[test]
    fn test_dollar_single_quote_escapes() {
        let tokens = tokenize("$'\\t\\r\\a\\b\\\\\\\"\\''");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![WordPart::DollarSingleQuoted(
                    "\t\r\x07\x08\\\"'".to_string()
                )],
            })
        );
    }

    #[test]
    fn test_mixed_quoting_in_word() {
        let tokens = tokenize("he\"ll\"o");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![
                    WordPart::Literal("he".to_string()),
                    WordPart::DoubleQuoted(vec![WordPart::Literal("ll".to_string())]),
                    WordPart::Literal("o".to_string()),
                ],
            })
        );
    }

    #[test]
    fn test_unterminated_single_quote() {
        let mut lexer = Lexer::new("echo 'hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, ShellErrorKind::UnterminatedSingleQuote);
    }

    #[test]
    fn test_unterminated_double_quote() {
        let mut lexer = Lexer::new("echo \"hello");
        let _ = lexer.next_token().unwrap();
        let err = lexer.next_token().unwrap_err();
        assert_eq!(err.kind, ShellErrorKind::UnterminatedDoubleQuote);
    }

    // ---- Task 6 tests ----

    #[test]
    fn test_simple_param() {
        let tokens = tokenize("$name");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Simple("name".to_string()))]
            })]
        );
    }

    #[test]
    fn test_param_in_word() {
        let tokens = tokenize("hello${x}world");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![
                    WordPart::Literal("hello".to_string()),
                    WordPart::Parameter(ParamExpr::Simple("x".to_string())),
                    WordPart::Literal("world".to_string()),
                ]
            })]
        );
    }

    #[test]
    fn test_positional_param() {
        let tokens = tokenize("$1 ${10}");
        assert_eq!(
            tokens[0],
            Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Positional(1))]
            })
        );
        assert_eq!(
            tokens[1],
            Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Positional(10))]
            })
        );
    }

    #[test]
    fn test_special_params() {
        let tokens = tokenize("$@ $* $# $? $- $$ $! $0");
        let expected = vec![
            SpecialParam::At,
            SpecialParam::Star,
            SpecialParam::Hash,
            SpecialParam::Question,
            SpecialParam::Dash,
            SpecialParam::Dollar,
            SpecialParam::Bang,
            SpecialParam::Zero,
        ];
        for (i, sp) in expected.into_iter().enumerate() {
            assert_eq!(
                tokens[i],
                Token::Word(Word {
                    parts: vec![WordPart::Parameter(ParamExpr::Special(sp))]
                })
            );
        }
    }

    #[test]
    fn test_param_default() {
        let tokens = tokenize("${x:-default}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Default {
                    name: "x".to_string(),
                    word: Some(Word::literal("default")),
                    null_check: true,
                })]
            })]
        );
    }

    #[test]
    fn test_param_default_no_colon() {
        let tokens = tokenize("${x-default}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Default {
                    name: "x".to_string(),
                    word: Some(Word::literal("default")),
                    null_check: false,
                })]
            })]
        );
    }

    #[test]
    fn test_param_length() {
        let tokens = tokenize("${#name}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::Length("name".to_string()))]
            })]
        );
    }

    #[test]
    fn test_param_strip_suffix() {
        let tokens = tokenize("${name%.txt}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::StripShortSuffix(
                    "name".to_string(),
                    Word::literal(".txt")
                ))]
            })]
        );
    }

    #[test]
    fn test_param_strip_long_prefix() {
        let tokens = tokenize("${name##*/}");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::Parameter(ParamExpr::StripLongPrefix(
                    "name".to_string(),
                    Word::literal("*/")
                ))]
            })]
        );
    }

    #[test]
    fn test_command_sub_dollar_paren() {
        let tokens = tokenize("$(echo hello)");
        assert_eq!(tokens.len(), 1);
        if let Token::Word(w) = &tokens[0] {
            assert_eq!(w.parts.len(), 1);
            assert!(matches!(&w.parts[0], WordPart::CommandSub(_)));
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn test_arith_expansion() {
        let tokens = tokenize("$((1 + 2))");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::ArithSub("1 + 2".to_string())]
            })]
        );
    }

    #[test]
    fn test_backtick_command_sub() {
        let tokens = tokenize("`echo hello`");
        assert_eq!(tokens.len(), 1);
        if let Token::Word(w) = &tokens[0] {
            assert!(matches!(&w.parts[0], WordPart::CommandSub(_)));
        } else {
            panic!("expected word");
        }
    }

    #[test]
    fn test_dollar_in_double_quotes() {
        let tokens = tokenize("\"hello $name\"");
        assert_eq!(
            tokens,
            vec![Token::Word(Word {
                parts: vec![WordPart::DoubleQuoted(vec![
                    WordPart::Literal("hello ".to_string()),
                    WordPart::Parameter(ParamExpr::Simple("name".to_string())),
                ])]
            })]
        );
    }

    // ---- Task 7 tests ----

    #[test]
    fn test_io_number_redirect() {
        let tokens = tokenize("2>/dev/null");
        assert_eq!(
            tokens,
            vec![
                Token::IoNumber(2),
                Token::Great,
                Token::Word(Word::literal("/dev/null"))
            ]
        );
    }

    #[test]
    fn test_io_number_input() {
        let tokens = tokenize("0<input.txt");
        assert_eq!(
            tokens,
            vec![
                Token::IoNumber(0),
                Token::Less,
                Token::Word(Word::literal("input.txt"))
            ]
        );
    }

    #[test]
    fn test_digits_not_followed_by_redirect() {
        let tokens = tokenize("123 abc");
        assert_eq!(
            tokens,
            vec![
                Token::Word(Word::literal("123")),
                Token::Word(Word::literal("abc"))
            ]
        );
    }

    #[test]
    fn test_fd_dup() {
        let tokens = tokenize("2>&1");
        assert_eq!(
            tokens,
            vec![
                Token::IoNumber(2),
                Token::GreatAnd,
                Token::Word(Word::literal("1"))
            ]
        );
    }
}
