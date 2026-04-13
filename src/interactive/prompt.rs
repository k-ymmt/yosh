use super::display_width::display_width;
use crate::env::ShellEnv;
use crate::expand::expand_word_to_string;
use crate::lexer::Lexer;
use crate::lexer::token::Token;
use crate::parser::ast::Word;

/// Return the POSIX default prompt for the given variable name.
fn default_prompt(var_name: &str) -> &'static str {
    match var_name {
        "PS1" => {
            // SAFETY: getuid() is always safe to call.
            if unsafe { libc::getuid() } == 0 {
                "# "
            } else {
                "$ "
            }
        }
        "PS2" => "> ",
        _ => "",
    }
}

/// Parse a raw prompt string as a double-quoted Word so that the expander
/// will handle `$VAR`, `${VAR}`, `$(cmd)`, etc.
///
/// We wrap the raw value in `"..."` and feed it to the lexer, which returns
/// a `Token::Word` whose parts come from the double-quoted context.
fn parse_prompt_word(raw: &str) -> Word {
    // Build a double-quoted string for the lexer.
    let input = format!("\"{}\"", raw);
    let mut lexer = Lexer::new(&input);
    match lexer.next_token() {
        Ok(tok) => {
            if let Token::Word(word) = tok.token {
                word
            } else {
                // Unexpected token type — fall back to literal
                Word::literal(raw)
            }
        }
        Err(_) => {
            // Parse failure — fall back to literal
            Word::literal(raw)
        }
    }
}

/// Expand a prompt variable (PS1 or PS2) through the shell's word expander.
///
/// If the variable is not set, the POSIX default is used.
/// The raw value is parsed as a double-quoted word so that parameter
/// expansion, command substitution, and arithmetic expansion are performed.
pub fn expand_prompt(env: &mut ShellEnv, var_name: &str) -> String {
    // 1. Get the raw value, or use the default.
    let raw = match env.vars.get(var_name) {
        Some(v) => v.to_string(),
        None => return default_prompt(var_name).to_string(),
    };

    // 2. Empty string => empty prompt.
    if raw.is_empty() {
        return String::new();
    }

    // 3. Parse the prompt string as a double-quoted word.
    let word = parse_prompt_word(&raw);

    // 4. Expand via expand_word_to_string (no field splitting / glob).
    //    Prompt expansion errors are non-fatal: fall back to the raw value.
    expand_word_to_string(env, &word).unwrap_or(raw)
}

/// Decomposed prompt for multi-line support.
pub struct PromptInfo {
    /// Lines above the editing line (display-only, printed once).
    pub upper_lines: Vec<String>,
    /// The final line displayed left of the input buffer.
    pub last_line: String,
    /// Display width of `last_line` (ANSI-stripped, Unicode-aware).
    pub last_line_width: usize,
}

impl PromptInfo {
    pub fn from_prompt(prompt: &str) -> Self {
        let lines: Vec<&str> = prompt.split('\n').collect();
        if lines.len() <= 1 {
            let last_line = prompt.to_string();
            let last_line_width = display_width(&last_line);
            PromptInfo {
                upper_lines: Vec::new(),
                last_line,
                last_line_width,
            }
        } else {
            let upper_lines: Vec<String> = lines[..lines.len() - 1]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let last_line = lines[lines.len() - 1].to_string();
            let last_line_width = display_width(&last_line);
            PromptInfo {
                upper_lines,
                last_line,
                last_line_width,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_prompt() {
        let info = PromptInfo::from_prompt("$ ");
        assert!(info.upper_lines.is_empty());
        assert_eq!(info.last_line, "$ ");
        assert_eq!(info.last_line_width, 2);
    }

    #[test]
    fn multi_line_prompt() {
        let info = PromptInfo::from_prompt("~/proj  main\n❯ ");
        assert_eq!(info.upper_lines, vec!["~/proj  main"]);
        assert_eq!(info.last_line, "❯ ");
        assert_eq!(info.last_line_width, 2); // ❯(1) + space(1)
    }

    #[test]
    fn multi_line_with_ansi() {
        let prompt = "\x1b[34m~/proj\x1b[0m \x1b[32m main\x1b[0m\n\x1b[1;35m❯\x1b[0m ";
        let info = PromptInfo::from_prompt(prompt);
        assert_eq!(info.upper_lines.len(), 1);
        assert_eq!(info.upper_lines[0], "\x1b[34m~/proj\x1b[0m \x1b[32m main\x1b[0m");
        assert_eq!(info.last_line, "\x1b[1;35m❯\x1b[0m ");
        assert_eq!(info.last_line_width, 2);
    }

    #[test]
    fn three_line_prompt() {
        let info = PromptInfo::from_prompt("line1\nline2\n$ ");
        assert_eq!(info.upper_lines, vec!["line1", "line2"]);
        assert_eq!(info.last_line, "$ ");
        assert_eq!(info.last_line_width, 2);
    }

    #[test]
    fn empty_prompt() {
        let info = PromptInfo::from_prompt("");
        assert!(info.upper_lines.is_empty());
        assert_eq!(info.last_line, "");
        assert_eq!(info.last_line_width, 0);
    }
}
