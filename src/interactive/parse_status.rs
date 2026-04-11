use crate::env::aliases::AliasStore;
use crate::error::{ShellErrorKind, ParseErrorKind};
use crate::parser::Parser;
use crate::parser::ast::CompleteCommand;

#[derive(Debug)]
pub enum ParseStatus {
    Complete(Vec<CompleteCommand>),
    Incomplete,
    Empty,
    Error(String),
}

/// Closing keywords used to probe whether an `UnexpectedToken`-at-EOF error
/// represents genuinely incomplete input (e.g. missing `fi`, `done`, `esac`,
/// `}`, `)`) rather than a real syntax error.
const CLOSING_KEYWORDS: &[&str] = &[
    "\nfi\n",
    "\ndone\n",
    "\nesac\n",
    "\n}\n",
    "\n)\n",
    "\n;;\nesac\n",
];

pub fn classify_parse(input: &str, aliases: &AliasStore) -> ParseStatus {
    // 1. If input is only whitespace/newlines -> Empty
    if input.trim().is_empty() {
        return ParseStatus::Empty;
    }

    // 2. If input ends with backslash-newline -> Incomplete
    if input.ends_with("\\\n") {
        return ParseStatus::Incomplete;
    }

    // 3. If input ends with | or && or || (trimmed) -> Incomplete
    let trimmed = input.trim_end_matches('\n').trim_end();
    if trimmed.ends_with('|') || trimmed.ends_with("&&") || trimmed.ends_with("||") {
        return ParseStatus::Incomplete;
    }

    // 4. Try parsing with Parser::new_with_aliases()
    let mut parser = Parser::new_with_aliases(input, aliases);
    let mut commands = Vec::new();

    // Skip leading newlines
    if parser.is_at_end() {
        return ParseStatus::Empty;
    }

    loop {
        // Skip newlines between commands
        while !parser.is_at_end()
            && parser.current_token() == &crate::lexer::token::Token::Newline
        {
            if let Err(e) = parser.advance() {
                if is_incomplete_error(&e.kind) {
                    return ParseStatus::Incomplete;
                }
                return ParseStatus::Error(e.message);
            }
        }

        if parser.is_at_end() {
            break;
        }

        match parser.parse_complete_command() {
            Ok(cmd) => {
                commands.push(cmd);
            }
            Err(e) => {
                if is_incomplete_error(&e.kind) {
                    return ParseStatus::Incomplete;
                }
                // If the parser hit an UnexpectedToken at EOF, determine
                // whether the input is structurally incomplete (e.g. missing
                // `fi`) or truly invalid.  We probe by appending closing
                // keywords and re-parsing; if any probe succeeds the input
                // was merely incomplete.
                if e.kind == ShellErrorKind::Parse(ParseErrorKind::UnexpectedToken) && parser.is_at_end() {
                    if is_completable(input, aliases) {
                        return ParseStatus::Incomplete;
                    }
                }
                return ParseStatus::Error(e.message);
            }
        }
    }

    // 5. If no commands collected -> Empty
    if commands.is_empty() {
        return ParseStatus::Empty;
    }

    // 6. Otherwise -> Complete(commands)
    ParseStatus::Complete(commands)
}

/// Check whether appending a closing keyword makes the input parseable,
/// which indicates the original input was incomplete rather than erroneous.
fn is_completable(input: &str, aliases: &AliasStore) -> bool {
    for suffix in CLOSING_KEYWORDS {
        let candidate = format!("{}{}", input, suffix);
        let mut p = Parser::new_with_aliases(&candidate, aliases);
        if p.parse_program().is_ok() {
            return true;
        }
    }
    false
}

fn is_incomplete_error(kind: &ShellErrorKind) -> bool {
    matches!(
        kind,
        ShellErrorKind::Parse(ParseErrorKind::UnterminatedSingleQuote)
            | ShellErrorKind::Parse(ParseErrorKind::UnterminatedDoubleQuote)
            | ShellErrorKind::Parse(ParseErrorKind::UnterminatedCommandSub)
            | ShellErrorKind::Parse(ParseErrorKind::UnterminatedArithSub)
            | ShellErrorKind::Parse(ParseErrorKind::UnterminatedParamExpansion)
            | ShellErrorKind::Parse(ParseErrorKind::UnterminatedBacktick)
            | ShellErrorKind::Parse(ParseErrorKind::UnterminatedDollarSingleQuote)
            | ShellErrorKind::Parse(ParseErrorKind::UnexpectedEof)
    )
}
