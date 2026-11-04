//! How more input relates to a pending command the text read so far
//! leaves incomplete: whether a parse error means more input may complete
//! it, and — for the command loop's closer-free batching — whether a line
//! of input can possibly complete it.

use crate::env;
use crate::error::{ParseErrorKind, ShellErrorKind};
use crate::parser::Parser;

/// Whether a parse error means the text ended in the middle of a command
/// (so more input may complete it) rather than a real syntax error.
pub(super) fn is_incomplete(p: &Parser, kind: &ShellErrorKind) -> bool {
    p.is_at_end()
        || matches!(
            kind,
            ShellErrorKind::Parse(
                ParseErrorKind::UnterminatedSingleQuote
                    | ParseErrorKind::UnterminatedDoubleQuote
                    | ParseErrorKind::UnterminatedCommandSub
                    | ParseErrorKind::UnterminatedArithSub
                    | ParseErrorKind::UnterminatedParamExpansion
                    | ParseErrorKind::UnterminatedBacktick
                    | ParseErrorKind::UnterminatedDollarSingleQuote
                    | ParseErrorKind::UnexpectedEof
                    | ParseErrorKind::InvalidHereDoc
            )
        )
}

/// Whether a parse error is the lexer's: the text ended inside a quote,
/// an expansion or a here-document body. More input then continues that
/// construct, whose end may be any line (a here-document delimiter).
pub(super) fn is_lexer_incomplete(kind: &ShellErrorKind) -> bool {
    matches!(
        kind,
        ShellErrorKind::Parse(
            ParseErrorKind::UnterminatedSingleQuote
                | ParseErrorKind::UnterminatedDoubleQuote
                | ParseErrorKind::UnterminatedCommandSub
                | ParseErrorKind::UnterminatedArithSub
                | ParseErrorKind::UnterminatedParamExpansion
                | ParseErrorKind::UnterminatedBacktick
                | ParseErrorKind::UnterminatedDollarSingleQuote
                | ParseErrorKind::InvalidHereDoc
        )
    )
}

/// Bytes that end a word, so the next byte may start a token: the
/// lexer's delimiters, plus NUL (which separates commands in a script).
fn is_word_delimiter(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | 0
    )
}

/// Whether `line` (one line of input, read while the pending command is
/// still inside an unclosed compound command) might complete that
/// command. Errs towards true: a false answer must be certain.
///
/// Only a closing token completes a compound command: `}`, `)`, `fi`,
/// `done` or `esac` (no alias can supply one, see `aliases_cannot_close`).
/// The line is scanned as if it started at the lexer's top level — every
/// line passed here is, as long as the lines before it got false: those
/// hold no quotes or backslashes (which could continue a word onto the
/// next line), no here-document operators (whose body lines are
/// arbitrary), and leave no parenthesis or `${` open at their end.
/// Within such a line a `)` that closes a construct opened on an earlier
/// line drives the parenthesis depth below zero, and a reserved word
/// starts right after a delimiter.
pub(super) fn line_may_close(line: &[u8]) -> bool {
    // `(`/`)` nesting outside `${...}`, and `${...}` nesting (bare
    // braces inside one counted too, so the span covers whatever the
    // lexer takes for the expansion).
    let (mut parens, mut braces) = (0i32, 0i32);
    let mut i = 0;
    while i < line.len() {
        let b = line[i];
        let token_start = i == 0 || is_word_delimiter(line[i - 1]);
        match b {
            b'\'' | b'"' | b'`' | b'\\' => return true,
            b'<' if line.get(i + 1) == Some(&b'<') => return true,
            b'$' if line.get(i + 1) == Some(&b'{') => {
                braces += 1;
                i += 2;
                continue;
            }
            b'{' if braces > 0 => braces += 1,
            b'}' if token_start => return true,
            b'}' if braces > 0 => braces -= 1,
            // A parenthesis inside `${...}` may be pattern text, which
            // the depth must not count.
            b'(' | b')' if braces > 0 => return true,
            b'(' => parens += 1,
            b')' => {
                parens -= 1;
                if parens < 0 {
                    return true;
                }
            }
            // A comment runs to the end of the line; inside an open
            // `$(` it would hide the `)`.
            b'#' if token_start => return parens != 0 || braces != 0,
            b'f' | b'd' | b'e' if token_start => {
                for word in [&b"fi"[..], b"done", b"esac"] {
                    if line[i..].starts_with(word)
                        && line
                            .get(i + word.len())
                            .is_none_or(|&c| is_word_delimiter(c))
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    parens != 0 || braces != 0
}

/// Whether no alias value can bring a closing token (or anything else
/// `line_may_close` looks for) into a line: substituting such values
/// keeps every verdict of `line_may_close` true to the line's tokens.
/// Nor may a value end in an operator or `)` (`alias a='true &&'`,
/// `alias d='f()'`), or be blank (`true && b` with `alias b=''`): a line
/// ending with such an alias may leave the command waiting for any next
/// line, not a closer, which `awaits_closer` judges from the raw text.
pub(super) fn aliases_cannot_close(aliases: &env::aliases::AliasStore) -> bool {
    aliases.is_empty()
        || aliases.sorted_iter().iter().all(|(_, value)| {
            !value.contains('#')
                && !matches!(
                    value.trim_end_matches([' ', '\t', '\n']).as_bytes().last(),
                    None | Some(b'&' | b'|' | b'!' | b'<' | b'>' | b'(' | b')' | b'\\')
                )
                && value.split('\n').all(|l| !line_may_close(l.as_bytes()))
        })
}

/// Whether the pending command `text` — incomplete, the parser having
/// reached the end of it without a lexer error — can only be completed by
/// a closing token, i.e. it sits inside an unclosed compound command
/// rather than after an operator (`&&`, `||`, `|`, `!`) that any next
/// line could follow. `completes_with_line` parses the text with one more
/// simple command (`:`) appended and says whether that completes it; it
/// is used only when a cheap look at the last line cannot tell (a
/// comment there, or here-documents whose bodies follow the operator).
pub(super) fn awaits_closer(text: &str, completes_with_line: impl FnOnce() -> bool) -> bool {
    let bytes = text.as_bytes();
    if !bytes.ends_with(b"\n") {
        return false;
    }
    if bytes.windows(2).any(|w| w == b"<<") {
        return !completes_with_line();
    }
    // The last line holding a token: after an operator only blank lines
    // and comments may follow.
    let Some(last) = bytes
        .split(|&b| b == b'\n')
        .rev()
        .map(|l| {
            let end = l.iter().rposition(|&b| b != b' ' && b != b'\t');
            &l[..end.map_or(0, |e| e + 1)]
        })
        .find(|l| !l.is_empty())
    else {
        return false;
    };
    let comment = last
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'#' && (i == 0 || is_word_delimiter(last[i - 1])));
    if comment {
        return !completes_with_line();
    }
    // After `f()` the body may be any compound command, `( )` included,
    // whose balanced parentheses `line_may_close` does not count.
    !matches!(
        last.last(),
        Some(b'\\' | b'&' | b'|' | b'!' | b'<' | b'>' | b')')
    )
}

/// Whether `text` followed by the line `:` parses as a complete command
/// (the probe of `awaits_closer`).
pub(super) fn completes_with_line(
    text: &str,
    aliases: &env::aliases::AliasStore,
    line: usize,
) -> bool {
    let probe = format!("{text}:\n");
    let mut p = Parser::resuming_at_line(&probe, aliases, line, None);
    let parsed = p.skip_newlines().and_then(|()| {
        if p.is_at_end() {
            return Ok(false);
        }
        p.parse_complete_command().map(|_| true)
    });
    matches!(parsed, Ok(true)) && !p.is_at_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_may_close_flags_every_possible_closing_token() {
        for line in [
            "}\n",
            "  :; }\n",
            "a;}\n",
            ")\n",
            "echo a) b\n",
            "( a ) )\n",
            "fi\n",
            "x; fi;\n",
            "done\n",
            "done>f\n",
            "  esac\n",
            "\0fi\n",
            // Quotes, escapes and here-documents may continue a word or
            // a body onto the next line.
            "echo 'a\n",
            "echo \"a\n",
            "echo `a\n",
            "echo a\\\n",
            "echo \\}\n",
            "cat <<E\n",
            "x=$((1<<2))\n",
            // Openers left open at the end of the line.
            "x=$(echo a\n",
            "x=${y\n",
            "( a\n",
            "echo $(( 1 + (2\n",
            // Parentheses inside `${...}`, and a brace that may end it
            // or be a reserved word.
            "echo ${x#(} )\n",
            "echo ${x:-{} }\n",
            "echo ${x:- }\n",
            // A comment inside an open `$(`.
            "x=$(echo # )\n",
        ] {
            assert!(line_may_close(line.as_bytes()), "{line:?}");
        }
    }

    #[test]
    fn line_may_close_passes_lines_without_closing_tokens() {
        for line in [
            ":\n",
            "x=$((x + 1))\n",
            "echo ${x} ${y:-z} ${#x} $# ${x#*.}\n",
            "y=$(date) z=$( (echo) )\n",
            "if true; then\n",
            "for i in a b; do\n",
            "f() {\n",
            "while :; do {\n",
            "file=final fix=1 done_x=1 esacs=1 x=fi\n",
            "echo a}b {}\n",
            "# a comment with } ) fi 'quotes' and <<\n",
            "echo a # done ) }\n",
            "case $x in\n",
            "\n",
        ] {
            assert!(!line_may_close(line.as_bytes()), "{line:?}");
        }
    }

    #[test]
    fn aliases_that_could_close_disable_the_shortcut() {
        let mut aliases = env::aliases::AliasStore::default();
        assert!(aliases_cannot_close(&aliases));
        aliases.set("ll", "ls -l");
        assert!(aliases_cannot_close(&aliases));
        for value in [
            "}",
            "fi",
            "echo )",
            "x #",
            "echo 'a",
            "a\nb; }",
            "true &&",
            "x | ",
            "!",
            "f()",
            "",
            "  ",
            "ls $(pwd)",
        ] {
            let mut a = aliases.clone();
            a.set("bad", value);
            assert!(!aliases_cannot_close(&a), "{value:?}");
        }
    }

    /// `awaits_closer` with the real probe, for text that the parser
    /// reports as incomplete without a lexer error.
    fn awaits(text: &str) -> bool {
        let aliases = env::aliases::AliasStore::default();
        let mut p = Parser::resuming_at_line(text, &aliases, 1, None);
        let parsed = p.skip_newlines().and_then(|()| p.parse_complete_command());
        match parsed {
            Err(e) => assert!(
                p.is_at_end() && !is_lexer_incomplete(&e.kind),
                "{text:?}: {e}"
            ),
            Ok(_) => panic!("{text:?} parsed as complete"),
        }
        awaits_closer(text, || completes_with_line(text, &aliases, 1))
    }

    #[test]
    fn awaits_closer_inside_compound_commands() {
        for text in [
            "{\n",
            "{\n: a\n",
            "f() {\n",
            "if true\n",
            "if true; then\n  :\n",
            "while :; do\n",
            "for i in a b\n",
            "case x in\n",
            "case x in\n a) :;;\n",
            "(\n echo a\n",
            "{\n:\n# a comment\n",
            "{\n: a # b\n",
            "{\ncat <<E\nbody\nE\n",
            "{\n{ cat <<E; }\nbody\nE\n",
        ] {
            assert!(awaits(text), "{text:?}");
        }
    }

    #[test]
    fn any_line_may_complete_after_an_operator() {
        for text in [
            "true &&\n",
            "true ||\n\n",
            "true |\n",
            "{\n:\n} &&\n",
            "true && # comment\n",
            "true &&\n# comment\n",
            "true &&\n  # comment\n\n",
            "cat <<E &&\nbody\nE\n",
            "cat <<E |\nbody\nE\n",
            "true &&\\\n",
            // The body after `f()` may be a balanced `( )`.
            "f()\n",
        ] {
            assert!(!awaits(text), "{text:?}");
        }
    }

    #[test]
    fn here_document_bodies_and_quotes_are_lexer_incomplete() {
        // `awaits_closer` relies on these never reaching it: their next
        // line may be any line.
        let aliases = env::aliases::AliasStore::default();
        for text in [
            "{\ncat <<E\n",
            "{\ncat <<E\nbody\n",
            "{\ncat <<-E\n\tbody\n",
            "{\ncat <<E; cat <<F\nbody\nE\n",
            "{\necho 'a\n",
            "{\necho \"a\n",
            "{\nx=$(\n",
            "{\nx=${y\n",
            "{\necho `a\n",
        ] {
            let mut p = Parser::resuming_at_line(text, &aliases, 1, None);
            let err = p
                .skip_newlines()
                .and_then(|()| p.parse_complete_command())
                .expect_err(text);
            assert!(is_lexer_incomplete(&err.kind), "{text:?}: {err}");
        }
    }
}
