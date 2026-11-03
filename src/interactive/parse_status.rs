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

    // 2. A trailing line continuation (`\<newline>`) or a trailing
    //    `|` / `&&` / `||` leaves the command open, but the parser cannot
    //    report it: the lexer folds the continuation away and an operator
    //    followed by end of input is indistinguishable from a syntax
    //    error. Detect both with a quote-, escape- and comment-aware
    //    scan so `echo a\\`, `echo a\|` and `echo hi # x \` run at once.
    if ends_with_continuation(input) {
        return ParseStatus::Incomplete;
    }

    // 3. Try parsing with Parser::new_with_aliases()
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

    // 4. If no commands collected -> Empty
    if commands.is_empty() {
        return ParseStatus::Empty;
    }

    // 5. Otherwise -> Complete(commands)
    ParseStatus::Complete(commands)
}

/// Whether `input` ends in a way that continues the command on the next
/// line: an unquoted, unescaped backslash immediately before the final
/// newline (line continuation), or a trailing unquoted `|`, `&&` or `||`
/// operator (possibly followed by blanks, newlines or a comment).
///
/// The scan follows the lexer's quoting rules: `\` escapes the next
/// character outside single quotes (`echo a\\` and `echo a\|` end in
/// literal characters, not a continuation or a pipe), and `#` at the
/// start of an unquoted word opens a comment to end of line (`echo hi
/// # x \` ends in a comment). Input that ends inside quotes returns
/// `false` — the parser reports the unterminated quote itself.
fn ends_with_continuation(input: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut word_start = true;
    let mut in_comment = false;
    // Unquoted, unescaped operator characters since the last word
    // character, with blanks between them collapsed to one space.
    let mut tail_ops = String::new();
    // The most recent character was the newline of a `\<newline>`.
    let mut continuation = false;
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        continuation = false;
        if in_comment {
            if c == '\n' {
                in_comment = false;
                word_start = true;
            }
            continue;
        }
        if in_single {
            if c == '\'' {
                in_single = false;
            }
            continue;
        }
        if in_double {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_double = false,
                _ => {}
            }
            continue;
        }
        match c {
            '\\' => match chars.next() {
                Some('\n') => continuation = true,
                Some(_) => {
                    tail_ops.clear();
                    word_start = false;
                }
                None => {
                    tail_ops.clear();
                    word_start = false;
                }
            },
            '\'' => {
                in_single = true;
                tail_ops.clear();
                word_start = false;
            }
            '"' => {
                in_double = true;
                tail_ops.clear();
                word_start = false;
            }
            '#' if word_start => in_comment = true,
            ' ' | '\t' | '\n' => {
                if !tail_ops.is_empty() && !tail_ops.ends_with(' ') {
                    tail_ops.push(' ');
                }
                word_start = true;
            }
            '|' | '&' | ';' | '<' | '>' | '(' | ')' => {
                tail_ops.push(c);
                word_start = true;
            }
            _ => {
                tail_ops.clear();
                word_start = false;
            }
        }
    }
    if continuation {
        return true;
    }
    if in_single || in_double {
        return false;
    }
    let ops = tail_ops.trim_end();
    ops.ends_with("&&") || ops.ends_with("||") || (ops.ends_with('|') && !ops.ends_with(">|"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn incomplete(input: &str) -> bool {
        matches!(
            classify_parse(input, &AliasStore::default()),
            ParseStatus::Incomplete
        )
    }

    #[test]
    fn line_continuation_is_incomplete() {
        assert!(incomplete("echo a\\\n"));
        assert!(incomplete("echo a \\\n"));
    }

    #[test]
    fn escaped_backslash_is_not_continuation() {
        // `echo a\\` ends in a literal backslash (dash runs it at once).
        assert!(!incomplete("echo a\\\\\n"));
        assert!(incomplete("echo a\\\\\\\n"));
    }

    #[test]
    fn backslash_in_comment_is_not_continuation() {
        assert!(!incomplete("echo hi # x \\\n"));
        assert!(!incomplete("echo hi #|\n"));
        assert!(!incomplete("echo hi # a &&\n"));
    }

    #[test]
    fn escaped_or_quoted_operator_is_not_continuation() {
        assert!(!incomplete("echo a\\|\n"));
        assert!(!incomplete("echo a\\&&\n"));
        assert!(!incomplete("echo 'a|'\n"));
        assert!(!incomplete("echo \"a&&\"\n"));
        // `#` mid-word is literal, so the `|` after it is a real pipe.
        assert!(incomplete("echo a#|\n"));
    }

    #[test]
    fn trailing_operators_are_incomplete() {
        assert!(incomplete("echo a |\n"));
        assert!(incomplete("echo a|\n"));
        assert!(incomplete("true &&\n"));
        assert!(incomplete("false ||\n"));
        assert!(incomplete("echo a | # comment\n"));
        assert!(incomplete("echo a\\\\|\n"));
        assert!(incomplete("echo a |\n\n"));
    }

    #[test]
    fn single_ampersand_and_clobber_are_not_continuation() {
        assert!(!incomplete("sleep 0 &\n"));
        assert!(!matches!(
            classify_parse("echo a >|\n", &AliasStore::default()),
            ParseStatus::Incomplete
        ));
    }
}
