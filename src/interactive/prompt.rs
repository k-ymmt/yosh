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
    expand_word_to_string(env, &word)
}
