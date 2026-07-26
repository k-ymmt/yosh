use crate::env::aliases::AliasStore;
use crate::error::{ParseErrorKind, ShellErrorKind};
use crate::parser::Parser;
use crate::parser::ast::CompleteCommand;

#[derive(Debug)]
pub enum ParseStatus {
    Complete(Vec<CompleteCommand>),
    Incomplete,
    Empty,
    Error(String),
}

/// Closing-keyword probes for `is_completable`: each suffix wraps a
/// single `:` null builtin (POSIX-defined, always valid) before the
/// closer, so the probe satisfies the non-empty `compound_list` rule
/// introduced in commit `fe7c31c` (2026-04-19). Without the `:` body,
/// every probe would produce an empty `then`/`do`/`else` body and fail
/// with `syntax error: empty compound list in <ctx>`, making genuinely
/// incomplete input indistinguishable from genuinely invalid input.
const CLOSING_KEYWORDS: &[&str] = &[
    "\n:\nfi\n",
    "\n:\ndone\n",
    "\n:\nesac\n",
    "\n:\n}\n",
    "\n:\n)\n",
    "\n:\n;;\nesac\n",
    // `do :\ndone` covers header-only `for x in ...\n` and `while cond\n`
    // inputs where `do` itself has not been typed yet.
    "\ndo :\ndone\n",
    // `then :\nfi` covers header-only `if cond\n` (and `elif cond\n`)
    // inputs where `then` itself has not been typed yet.
    "\nthen :\nfi\n",
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

    // 3. If input ends with | or && or || (trimmed) -> Incomplete.
    //    Comments are stripped first so a trailing operator *inside* a
    //    comment (`echo hi #|`) doesn't classify as Incomplete forever.
    let stripped = strip_comments(input);
    let trimmed = stripped.trim_end_matches('\n').trim_end();
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
        while !parser.is_at_end() && parser.current_token() == &crate::lexer::token::Token::Newline
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
                if e.kind == ShellErrorKind::Parse(ParseErrorKind::UnexpectedToken)
                    && parser.is_at_end()
                    && is_completable(input, aliases)
                {
                    return ParseStatus::Incomplete;
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

/// Remove comment text from `input` for the textual trailing-operator
/// check above. Quote- and backslash-aware: `#` opens a comment only when
/// unquoted and at the start of a word (input start, or after whitespace
/// or an operator/redirect character), and runs to the end of the line.
/// The terminating newline is kept so line structure is preserved.
fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut word_start = true;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single => {
                out.push(c);
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
                word_start = false;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                out.push(c);
                word_start = false;
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push(c);
                word_start = false;
            }
            '#' if !in_single && !in_double && word_start => {
                for rest in chars.by_ref() {
                    if rest == '\n' {
                        out.push('\n');
                        break;
                    }
                }
                word_start = true;
            }
            c if c.is_whitespace() => {
                out.push(c);
                word_start = true;
            }
            '|' | '&' | ';' | '(' | ')' | '<' | '>' => {
                out.push(c);
                word_start = true;
            }
            c => {
                out.push(c);
                word_start = false;
            }
        }
    }
    out
}

/// Probe depth for `is_completable`: each level appends one closing-keyword
/// suffix, so depth N resolves N nested header-only constructs (e.g.
/// `while true\nif x\n` needs `then :\nfi` and then `do :\ndone`). Nesting
/// more than 3 unfinished headers is rare enough that classifying deeper
/// input as Error is acceptable.
const MAX_PROBE_DEPTH: usize = 3;

/// Check whether appending closing keywords makes the input parseable,
/// which indicates the original input was incomplete rather than erroneous.
/// Probes compose (bounded by `MAX_PROBE_DEPTH`): a suffix that closes the
/// innermost construct but leaves an outer construct open recurses on the
/// partially closed candidate.
fn is_completable(input: &str, aliases: &AliasStore) -> bool {
    is_completable_at_depth(input, aliases, MAX_PROBE_DEPTH)
}

fn is_completable_at_depth(input: &str, aliases: &AliasStore, depth: usize) -> bool {
    for suffix in CLOSING_KEYWORDS {
        let candidate = format!("{}{}", input, suffix);
        let mut p = Parser::new_with_aliases(&candidate, aliases);
        match p.parse_program() {
            Ok(_) => return true,
            Err(e) => {
                // Input plus a closer parses as merely-incomplete: the
                // original input was incomplete (the probe suffixes cannot
                // themselves introduce unterminated constructs).
                if is_incomplete_error(&e.kind) {
                    return true;
                }
                if depth > 1
                    && e.kind == ShellErrorKind::Parse(ParseErrorKind::UnexpectedToken)
                    && p.is_at_end()
                    && is_completable_at_depth(&candidate, aliases, depth - 1)
                {
                    return true;
                }
            }
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
            // An unclosed heredoc body means the user is still typing it.
            | ShellErrorKind::Parse(ParseErrorKind::InvalidHereDoc)
    )
}
