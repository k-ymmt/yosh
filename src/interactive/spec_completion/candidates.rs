//! Candidate generation: turn a [`Resolution`] into the candidate list
//! for the cursor word.

use super::{
    BuiltinType, CandidateSource, EXEC_TIMEOUT, Resolution, SpecStore, command_words, resolve,
    run_exec,
};
use crate::interactive::command_completion::CommandCompletionContext;
use crate::interactive::completion::{self, CompletionContext};

/// The outcome of a spec-based completion attempt.
pub struct SpecCompletion {
    pub candidates: Vec<String>,
    pub common_prefix: String,
    /// Re-inserted verbatim before the selected candidate: a `--flag=`
    /// prefix or the directory part of a path.
    pub keep_prefix: String,
}

/// Try spec-based completion for `word`, which starts at `word_start`.
///
/// Returns `None` when there is no spec or the spec gives no guidance
/// (including the fallback rule: a source that produced zero candidates)
/// — the caller should then run its existing path completion. Returns
/// `Some` with empty candidates only for `type = "none"` suppression.
pub fn try_complete(
    buf: &str,
    word_start: usize,
    word: &str,
    store: &mut SpecStore,
    ctx: &CompletionContext,
    cmd_ctx: &mut CommandCompletionContext<'_>,
) -> Option<SpecCompletion> {
    let (cmd, prior_words) = command_words(buf, word_start)?;
    // Snapshot before `store.get`: its returned reference borrows `store`
    // for the rest of this function, so `store.exec_env` must be read first.
    let exec_env = store.exec_env.clone();
    let spec = store.get(&cmd)?;
    let (resolution, keep_prefix) = resolve(spec, &prior_words, word);
    let filter = &word[keep_prefix.len()..];

    match resolution {
        Resolution::FlagNames(names) => finish(sorted_matches(names, word), String::new()),
        Resolution::FlagValue(source) => complete_source(
            source,
            filter,
            keep_prefix,
            &[],
            ctx,
            cmd_ctx,
            exec_env.as_deref(),
        ),
        Resolution::Positional {
            subcommands,
            flags,
            source,
        } => {
            // Fixed candidate order: subcommand names, then flag
            // spellings, then source candidates (appended by
            // complete_source). Each group sorts alphabetically.
            let mut static_matches = sorted_matches(subcommands, filter);
            static_matches.extend(sorted_matches(flags, filter));
            match source {
                Some(source) => complete_source(
                    source,
                    filter,
                    keep_prefix,
                    &static_matches,
                    ctx,
                    cmd_ctx,
                    exec_env.as_deref(),
                ),
                None => finish(static_matches, keep_prefix),
            }
        }
    }
}

/// The entries of `items` that start with `prefix`, sorted.
fn sorted_matches(items: Vec<String>, prefix: &str) -> Vec<String> {
    let mut matches: Vec<String> = items
        .into_iter()
        .filter(|item| item.starts_with(prefix))
        .collect();
    matches.sort();
    matches
}

/// Path-complete an arbitrary word string (used for flag values where
/// the word to complete is the part after `--flag=`, which the
/// buffer-based `completion::complete` cannot see in isolation).
/// Returns the candidates and the word's directory prefix (kept
/// verbatim on insertion).
fn complete_path_word(word: &str, ctx: &CompletionContext) -> (Vec<String>, String) {
    let (dir_part, prefix) = completion::split_path(word, &ctx.home);
    let resolved_dir = completion::resolve_dir(&dir_part, &ctx.cwd);
    let candidates = completion::generate_candidates(&resolved_dir, &prefix, ctx.show_dotfiles);
    (candidates, completion::dir_prefix_of(word))
}

/// Generate candidates for one source, prepend the already-filtered
/// static matches (subcommand names, then flag spellings), and apply
/// the fallback rule. Source candidates sort alphabetically after the
/// static group so the display order is subcommands → flags → rest.
fn complete_source(
    source: &CandidateSource,
    filter: &str,
    keep_prefix: String,
    static_matches: &[String],
    ctx: &CompletionContext,
    cmd_ctx: &mut CommandCompletionContext<'_>,
    exec_env: Option<&[(String, String)]>,
) -> Option<SpecCompletion> {
    let (mut candidates, keep_prefix) = match source {
        CandidateSource::Builtin(BuiltinType::None) => {
            // Suppression covers source candidates and the path-completion
            // fallback — statically declared sibling subcommands and flags
            // still complete. Empty static_matches keeps this a final
            // empty result.
            return Some(SpecCompletion {
                common_prefix: completion::longest_common_prefix(static_matches),
                candidates: static_matches.to_vec(),
                keep_prefix,
            });
        }
        CandidateSource::Builtin(BuiltinType::File) => {
            if static_matches.is_empty() && keep_prefix.is_empty() {
                // Pure file completion: defer to the caller's existing
                // path completion (identical behavior, fewer moving parts).
                return None;
            }
            let (cands, dir_prefix) = complete_path_word(filter, ctx);
            (cands, format!("{keep_prefix}{dir_prefix}"))
        }
        CandidateSource::Builtin(BuiltinType::Directory) => {
            let (cands, dir_prefix) = complete_path_word(filter, ctx);
            let dirs: Vec<String> = cands.into_iter().filter(|c| c.ends_with('/')).collect();
            (dirs, format!("{keep_prefix}{dir_prefix}"))
        }
        CandidateSource::Builtin(BuiltinType::Command) => {
            let cands =
                cmd_ctx
                    .completer
                    .complete(filter, cmd_ctx.path, cmd_ctx.builtins, cmd_ctx.aliases);
            (cands, keep_prefix)
        }
        CandidateSource::Values(values) => {
            let cands: Vec<String> = values
                .iter()
                .filter(|v| v.starts_with(filter))
                .cloned()
                .collect();
            (cands, keep_prefix)
        }
        CandidateSource::Exec(cmd) => {
            let cands: Vec<String> = run_exec(cmd, EXEC_TIMEOUT, exec_env)
                .into_iter()
                .filter(|c| c.starts_with(filter))
                .collect();
            (cands, keep_prefix)
        }
    };

    candidates.sort();
    candidates.dedup();
    candidates.retain(|c| !static_matches.contains(c));
    let mut ordered = static_matches.to_vec();
    ordered.extend(candidates);
    finish(ordered, keep_prefix)
}

/// Apply the fallback rule (empty → None) and compute the common prefix.
fn finish(candidates: Vec<String>, keep_prefix: String) -> Option<SpecCompletion> {
    if candidates.is_empty() {
        return None;
    }
    let common_prefix = completion::longest_common_prefix(&candidates);
    Some(SpecCompletion {
        candidates,
        common_prefix,
        keep_prefix,
    })
}
