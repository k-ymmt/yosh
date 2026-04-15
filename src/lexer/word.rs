use crate::error::{self, ShellError, ParseErrorKind};
use crate::parser::ast::{ParamExpr, SpecialParam, Word, WordPart};
use super::token::{Span, SpannedToken, Token};
use super::{Lexer, is_name_start, is_name_char};

impl Lexer {
    // ---- Word construction methods ----

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

    pub(crate) fn read_word(&mut self) -> error::Result<SpannedToken> {
        let span = self.current_span();
        let parts = self.read_word_parts(false, None)?;
        if parts.is_empty() {
            return Err(ShellError::parse(
                ParseErrorKind::UnexpectedToken,
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

    // ---- Quoting methods ----

    fn read_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut content = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedSingleQuote,
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
            return Err(ShellError::parse(
                ParseErrorKind::UnterminatedDoubleQuote,
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

    // ---- Identifier/parameter methods ----

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
            return Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
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
            _ => Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
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
            return Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("expected '{}'", expected as char),
            ));
        }
        self.advance();
        Ok(())
    }

    // ---- Parameter expansion methods ----

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
            return Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
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
            _ => Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("unexpected character in parameter expansion: '{}'", ch as char),
            )),
        }
    }

    /// Reads the conditional operator (`-`, `=`, `?`, `+`) and its word argument.
    fn read_conditional_param(&mut self, span: Span, name: String, null_check: bool) -> error::Result<WordPart> {
        if self.at_end() {
            return Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
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
            _ => return Err(ShellError::parse(
                ParseErrorKind::UnterminatedParamExpansion,
                span.line,
                span.column,
                format!("unknown parameter operator: '{}'", op as char),
            )),
        };
        Ok(WordPart::Parameter(expr))
    }

    // ---- Dollar/command substitution methods ----

    /// Handles `$'...'` with C-style escape sequences.
    /// Called after `$` is consumed; current byte is `'`.
    fn read_dollar_single_quote(&mut self) -> error::Result<WordPart> {
        let span = self.current_span();
        self.advance(); // consume opening '
        let mut content = String::new();
        loop {
            if self.at_end() {
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedDollarSingleQuote,
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
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedArithSub,
                    span.line,
                    span.column,
                    "unterminated arithmetic expansion",
                ));
            }
            let ch = self.current_byte();
            match ch {
                b'$' if self.peek_byte() == b'(' => {
                    // Nested $(cmd) or $((...)) — delegate to read_balanced_parens
                    // which correctly handles quotes inside command substitutions
                    let sub_span = self.current_span();
                    expr.push('$');
                    self.advance(); // consume '$', now at '('
                    let content = self.read_balanced_parens(sub_span)?;
                    expr.push('(');
                    expr.push_str(&content);
                    expr.push(')');
                }
                b'`' => {
                    // Backtick command substitution — skip to matching `
                    expr.push('`');
                    self.advance();
                    while !self.at_end() {
                        let bch = self.current_byte();
                        if bch == b'\\' {
                            expr.push('\\');
                            self.advance();
                            if !self.at_end() {
                                expr.push(self.current_byte() as char);
                                self.advance();
                            }
                        } else {
                            expr.push(bch as char);
                            self.advance();
                            if bch == b'`' {
                                break;
                            }
                        }
                    }
                }
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
                            return Err(ShellError::parse(
                                ParseErrorKind::UnterminatedArithSub,
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
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedCommandSub,
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
                            return Err(ShellError::parse(
                                ParseErrorKind::UnterminatedCommandSub,
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
                            return Err(ShellError::parse(
                                ParseErrorKind::UnterminatedCommandSub,
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
                return Err(ShellError::parse(
                    ParseErrorKind::UnterminatedBacktick,
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
}
