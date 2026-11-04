//! Matching engine: split the command line into words and walk them
//! down the spec tree to decide what the cursor word completes as.

use super::{CandidateSource, CompletionSpec, FlagSpec, Level};

/// What the word under the cursor should be completed as.
#[derive(Debug)]
pub enum Resolution<'a> {
    /// Complete a flag's value from this source.
    FlagValue(&'a CandidateSource),
    /// The current word starts with `-`: complete flag spellings.
    FlagNames(Vec<String>),
    /// Positional: subcommand names and flag spellings at the current
    /// level, plus the candidate source for the current positional
    /// index (if any). Flags are offered here so that an empty word
    /// lists them without requiring a leading `-`; a non-empty
    /// non-dash word never prefix-matches them.
    Positional {
        subcommands: Vec<String>,
        flags: Vec<String>,
        source: Option<&'a CandidateSource>,
    },
}

/// Walk `prior_words` (the words between the command name and the
/// cursor) down the spec tree and decide what `current_word` completes.
///
/// Returns the resolution plus `keep_prefix`: the leading part of the
/// current word to re-insert verbatim (non-empty only for `--flag=value`
/// forms, where it is `--flag=`).
pub fn resolve<'a>(
    spec: &'a CompletionSpec,
    prior_words: &[String],
    current_word: &str,
) -> (Resolution<'a>, String) {
    let mut level = spec.level();
    let mut positional_index = 0usize;
    let mut pending_value_flag: Option<&FlagSpec> = None;

    for word in prior_words {
        if pending_value_flag.take().is_some() {
            continue; // consumed as the pending flag's value
        }
        if let Some(sub) = level.subcommands.iter().find(|s| s.name == *word) {
            level = sub.level();
            positional_index = 0;
            continue;
        }
        if word.starts_with('-') {
            // `--flag=value` is self-contained; a bare value-taking flag
            // makes the NEXT word its value. Unknown flags are consumed
            // as booleans.
            let name = word.split('=').next().unwrap_or(word);
            if let Some(flag) = find_flag(level, name)
                && flag.value.is_some()
                && !word.contains('=')
            {
                pending_value_flag = Some(flag);
            }
            continue;
        }
        positional_index += 1;
    }

    if let Some(flag) = pending_value_flag {
        let source = flag.value.as_ref().expect("pending flag takes a value");
        return (Resolution::FlagValue(source), String::new());
    }

    if current_word.starts_with('-') {
        if let Some(eq) = current_word.find('=') {
            let name = &current_word[..eq];
            if let Some(flag) = find_flag(level, name)
                && let Some(source) = flag.value.as_ref()
            {
                return (
                    Resolution::FlagValue(source),
                    current_word[..=eq].to_string(),
                );
            }
        }
        let names: Vec<String> = level
            .flags
            .iter()
            .flat_map(|f| f.names.iter().cloned())
            .collect();
        return (Resolution::FlagNames(names), String::new());
    }

    let subcommands: Vec<String> = level.subcommands.iter().map(|s| s.name.clone()).collect();
    let flags: Vec<String> = level
        .flags
        .iter()
        .flat_map(|f| f.names.iter().cloned())
        .collect();
    let source = if level.args.is_empty() {
        None
    } else {
        Some(&level.args[positional_index.min(level.args.len() - 1)])
    };
    (
        Resolution::Positional {
            subcommands,
            flags,
            source,
        },
        String::new(),
    )
}

fn find_flag<'a>(level: Level<'a>, name: &str) -> Option<&'a FlagSpec> {
    level
        .flags
        .iter()
        .find(|f| f.names.iter().any(|n| n == name))
}

/// Split the current pipeline segment of `buf[..word_start]` into the
/// command word and the argument words before the cursor. Quote
/// characters wrapping a word are stripped; a leading `!` word is
/// skipped. Returns `None` when the cursor word is itself the command.
pub fn command_words(buf: &str, word_start: usize) -> Option<(String, Vec<String>)> {
    let end = word_start.min(buf.len());

    // Find the start of the current pipeline segment (shared delimiter
    // definition — `)`, `<`, `>` end a segment too, so the command word
    // never resolves against a closed subshell or a redirection target).
    let seg_start = crate::interactive::completion::segment_start(buf, end);

    // Parse words, treating quoted strings as single tokens
    let segment = &buf[seg_start..end];
    let mut words = Vec::new();
    let mut current_word = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for ch in segment.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                current_word.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current_word.push(ch);
            }
            ' ' | '\t' | '\n' if !in_single && !in_double => {
                if !current_word.is_empty() {
                    words.push(current_word.clone());
                    current_word.clear();
                }
            }
            _ => current_word.push(ch),
        }
    }
    if !current_word.is_empty() {
        words.push(current_word);
    }

    // Strip quotes from all words
    words = words
        .into_iter()
        .map(|w| w.trim_matches(|c| c == '\'' || c == '"').to_string())
        .collect();

    // Skip leading `!`
    if words.first().is_some_and(|w| w == "!") {
        words.remove(0);
    }

    if words.is_empty() {
        return None;
    }

    let cmd = words.remove(0);
    Some((cmd, words))
}
