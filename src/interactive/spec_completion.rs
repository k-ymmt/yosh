//! Spec-based tab completion: per-command TOML definition files.
//!
//! Users drop `<command>.toml` files into `~/.config/yosh/completions/`
//! to define subcommand / flag / argument completion for any command.
//! See `completion.md` at the repository root for the full design.

use serde::Deserialize;

// ── Schema ──────────────────────────────────────────────────────────

/// How to produce candidates. Deserialized from a TOML table that must
/// contain exactly one of `type` / `values` / `exec`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawSource")]
pub enum CandidateSource {
    /// A built-in generator (`type = "file"` etc.).
    Builtin(BuiltinType),
    /// A static candidate list (`values = [...]`).
    Values(Vec<String>),
    /// A shell command whose stdout lines become candidates (`exec = "..."`).
    Exec(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinType {
    File,
    Directory,
    Command,
    /// No candidates, and no fallback to path completion.
    None,
}

/// Raw deserialization target for [`CandidateSource`]; validated in the
/// `TryFrom` conversion so the exactly-one-key rule is a parse error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    #[serde(rename = "type")]
    builtin: Option<String>,
    values: Option<Vec<String>>,
    exec: Option<String>,
}

impl TryFrom<RawSource> for CandidateSource {
    type Error = String;

    fn try_from(raw: RawSource) -> Result<Self, String> {
        let keys =
            raw.builtin.is_some() as u8 + raw.values.is_some() as u8 + raw.exec.is_some() as u8;
        if keys != 1 {
            return Err(
                "candidate source must have exactly one of `type`, `values`, `exec`".to_string(),
            );
        }
        if let Some(t) = raw.builtin {
            let builtin = match t.as_str() {
                "file" => BuiltinType::File,
                "directory" => BuiltinType::Directory,
                "command" => BuiltinType::Command,
                "none" => BuiltinType::None,
                other => return Err(format!("unknown builtin type `{other}`")),
            };
            Ok(CandidateSource::Builtin(builtin))
        } else if let Some(values) = raw.values {
            Ok(CandidateSource::Values(values))
        } else {
            Ok(CandidateSource::Exec(raw.exec.expect("exec key present")))
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagSpec {
    /// All spellings of this flag, e.g. `["-m", "--message"]`.
    pub names: Vec<String>,
    /// Present iff the flag takes a value, completed from this source.
    #[serde(default)]
    pub value: Option<CandidateSource>,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionSpec {
    #[serde(default)]
    pub args: Vec<CandidateSource>,
    #[serde(default)]
    pub flags: Vec<FlagSpec>,
    #[serde(default)]
    pub subcommands: Vec<Subcommand>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subcommand {
    pub name: String,
    #[serde(default)]
    pub args: Vec<CandidateSource>,
    #[serde(default)]
    pub flags: Vec<FlagSpec>,
    #[serde(default)]
    pub subcommands: Vec<Subcommand>,
}

/// A borrowed view of one level of the spec tree, shared between the
/// top level (`CompletionSpec`) and nested `Subcommand`s.
#[derive(Clone, Copy)]
pub struct Level<'a> {
    pub args: &'a [CandidateSource],
    pub flags: &'a [FlagSpec],
    pub subcommands: &'a [Subcommand],
}

impl CompletionSpec {
    pub fn level(&self) -> Level<'_> {
        Level {
            args: &self.args,
            flags: &self.flags,
            subcommands: &self.subcommands,
        }
    }

    /// Parse and validate a completion spec from TOML text.
    pub fn parse(text: &str) -> Result<CompletionSpec, String> {
        let spec: CompletionSpec = toml::from_str(text).map_err(|e| e.to_string())?;
        validate_level(spec.level())?;
        Ok(spec)
    }
}

impl Subcommand {
    pub fn level(&self) -> Level<'_> {
        Level {
            args: &self.args,
            flags: &self.flags,
            subcommands: &self.subcommands,
        }
    }
}

/// Reject duplicate subcommand names and duplicate flag spellings within
/// one level, and flags with an empty `names` list. Recurses into
/// subcommands.
fn validate_level(level: Level<'_>) -> Result<(), String> {
    let mut sub_names = std::collections::HashSet::new();
    for sub in level.subcommands {
        if !sub_names.insert(sub.name.as_str()) {
            return Err(format!("duplicate subcommand `{}`", sub.name));
        }
        validate_level(sub.level())?;
    }
    let mut spellings = std::collections::HashSet::new();
    for flag in level.flags {
        if flag.names.is_empty() {
            return Err("flag with empty `names`".to_string());
        }
        for name in &flag.names {
            if !spellings.insert(name.as_str()) {
                return Err(format!("duplicate flag `{name}`"));
            }
        }
    }
    Ok(())
}

// ── SpecStore: lazy per-command loading ─────────────────────────────

/// Lazy loader and per-session cache for completion specs.
///
/// Specs live at `<dir>/<command>.toml`. Each command name is looked up
/// on disk at most once per session: both missing files and parse
/// failures are cached as `None`, which also makes the parse-error
/// warning naturally print only once.
pub struct SpecStore {
    dir: std::path::PathBuf,
    cache: std::collections::HashMap<String, Option<CompletionSpec>>,
    exec_env: Option<Vec<(String, String)>>,
}

impl SpecStore {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            cache: std::collections::HashMap::new(),
            exec_env: None,
        }
    }

    /// Per-prompt snapshot of the shell's exported variables, passed to
    /// `exec` candidate commands. `None` inherits the process env.
    pub fn set_exec_env(&mut self, env: Vec<(String, String)>) {
        self.exec_env = Some(env);
    }

    /// Store rooted at the standard location under `home`
    /// (`~/.config/yosh/completions`).
    pub fn from_home(home: &str) -> Self {
        Self::new(std::path::PathBuf::from(home).join(".config/yosh/completions"))
    }

    /// Look up the spec for `command`. Only the final path component is
    /// used: `/usr/bin/git` and `git` both resolve to `git.toml`.
    pub fn get(&mut self, command: &str) -> Option<&CompletionSpec> {
        let name = command.rsplit('/').next().unwrap_or(command);
        if name.is_empty() || name == "." || name == ".." {
            return None;
        }
        if !self.cache.contains_key(name) {
            let loaded = self.load(name);
            self.cache.insert(name.to_string(), loaded);
        }
        self.cache.get(name).and_then(|entry| entry.as_ref())
    }

    fn load(&self, name: &str) -> Option<CompletionSpec> {
        let path = self.dir.join(format!("{name}.toml"));
        let text = std::fs::read_to_string(&path).ok()?;
        match CompletionSpec::parse(&text) {
            Ok(spec) => Some(spec),
            Err(err) => {
                eprintln!("yosh: completion: {name}.toml: {err}");
                None
            }
        }
    }
}

// ── Matching engine ─────────────────────────────────────────────────

/// What the word under the cursor should be completed as.
#[derive(Debug)]
pub enum Resolution<'a> {
    /// Complete a flag's value from this source.
    FlagValue(&'a CandidateSource),
    /// The current word starts with `-`: complete flag spellings.
    FlagNames(Vec<String>),
    /// Positional: subcommand names at the current level, plus the
    /// candidate source for the current positional index (if any).
    Positional {
        subcommands: Vec<String>,
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
    let source = if level.args.is_empty() {
        None
    } else {
        Some(&level.args[positional_index.min(level.args.len() - 1)])
    };
    (
        Resolution::Positional {
            subcommands,
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
    let bytes = buf.as_bytes();
    let end = word_start.min(buf.len());
    let mut seg_start = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    // Find the start of the current pipeline segment
    for (i, &ch) in bytes.iter().enumerate().take(end) {
        match ch {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'|' | b';' | b'&' | b'(' if !in_single && !in_double => seg_start = i + 1,
            _ => {}
        }
    }

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

// ── exec runner ─────────────────────────────────────────────────────

/// Budget for `exec` candidate commands. Chosen to keep Tab latency
/// bounded; see completion.md.
pub const EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Run `sh -c <cmd>` and return its stdout lines (empty lines dropped).
/// A timeout kills the child; timeouts and non-zero exits both yield an
/// empty Vec. Runs as a child process so completion can never mutate
/// shell state.
///
/// `env`: `None` inherits the process environment (used by unit tests);
/// `Some(pairs)` runs the child with exactly `pairs` as its environment
/// (the shell's exported variables, snapshotted per-prompt).
pub fn run_exec(
    cmd: &str,
    timeout: std::time::Duration,
    env: Option<&[(String, String)]>,
) -> Vec<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(pairs) = env {
        command
            .env_clear()
            .envs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Vec::new(),
    };

    // Drain stdout on a helper thread so a pipe-buffer-filling child can
    // never deadlock the timeout loop below. Send the result over a
    // channel rather than joining: a backgrounded grandchild that
    // inherits the write end of the pipe can keep it open long after
    // this child exits, and joining unconditionally would then hang the
    // line editor.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    let Some(status) = status else {
        // Timed out: the reader thread detaches and dies with the pipe.
        return Vec::new();
    };
    if !status.success() {
        return Vec::new();
    }
    // The child exited, but a backgrounded grandchild may still hold the
    // pipe's write end and delay EOF forever — bound the wait by the
    // remaining budget instead of joining unconditionally.
    let remaining = deadline.saturating_duration_since(std::time::Instant::now())
        + std::time::Duration::from_millis(100);
    let out = match rx.recv_timeout(remaining) {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };
    out.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

// ── Candidate generation ────────────────────────────────────────────

use super::command_completion::CommandCompletionContext;
use super::completion::{self, CompletionContext};

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
        Resolution::FlagNames(names) => {
            let mut candidates: Vec<String> =
                names.into_iter().filter(|n| n.starts_with(word)).collect();
            candidates.sort();
            finish(candidates, String::new())
        }
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
            source,
        } => {
            let sub_matches: Vec<String> = subcommands
                .into_iter()
                .filter(|s| s.starts_with(filter))
                .collect();
            match source {
                Some(source) => complete_source(
                    source,
                    filter,
                    keep_prefix,
                    &sub_matches,
                    ctx,
                    cmd_ctx,
                    exec_env.as_deref(),
                ),
                None => finish(sub_matches, keep_prefix),
            }
        }
    }
}

/// Path-complete an arbitrary word string (used for flag values where
/// the word to complete is the part after `--flag=`, which the
/// buffer-based `completion::complete` cannot see in isolation).
/// Returns the candidates and the word's directory prefix (kept
/// verbatim on insertion).
fn complete_path_word(word: &str, ctx: &CompletionContext) -> (Vec<String>, String) {
    let (dir_part, prefix) = completion::split_path(word, &ctx.home);
    let resolved_dir = if dir_part.is_empty() {
        ctx.cwd.clone()
    } else if dir_part.starts_with('/') {
        dir_part.clone()
    } else {
        let mut path = std::path::PathBuf::from(&ctx.cwd);
        path.push(&dir_part);
        path.to_string_lossy().into_owned()
    };
    let candidates = completion::generate_candidates(&resolved_dir, prefix, ctx.show_dotfiles);
    let dir_prefix = match word.rfind('/') {
        Some(pos) => word[..=pos].to_string(),
        None => String::new(),
    };
    (candidates, dir_prefix)
}

/// Generate candidates for one source, merge in already-filtered
/// subcommand names, and apply the fallback rule.
fn complete_source(
    source: &CandidateSource,
    filter: &str,
    keep_prefix: String,
    sub_matches: &[String],
    ctx: &CompletionContext,
    cmd_ctx: &mut CommandCompletionContext<'_>,
    exec_env: Option<&[(String, String)]>,
) -> Option<SpecCompletion> {
    let (mut candidates, keep_prefix) = match source {
        CandidateSource::Builtin(BuiltinType::None) => {
            // Suppression covers source candidates and the path-completion
            // fallback — statically declared sibling subcommands still
            // complete. Empty sub_matches keeps this a final empty result.
            return Some(SpecCompletion {
                common_prefix: completion::longest_common_prefix(sub_matches),
                candidates: sub_matches.to_vec(),
                keep_prefix,
            });
        }
        CandidateSource::Builtin(BuiltinType::File) => {
            if sub_matches.is_empty() && keep_prefix.is_empty() {
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

    candidates.extend(sub_matches.iter().cloned());
    candidates.sort();
    candidates.dedup();
    finish(candidates, keep_prefix)
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── CompletionSpec::parse ────────────────────────────────────────

    const GIT_SPEC: &str = r#"
[[args]]
type = "file"

[[flags]]
names = ["-C"]
value = { type = "directory" }

[[flags]]
names = ["--no-pager"]

[[subcommands]]
name = "checkout"

[[subcommands.flags]]
names = ["-b"]
value = { type = "none" }

[[subcommands.args]]
exec = "git branch --format='%(refname:short)'"

[[subcommands]]
name = "remote"

[[subcommands.subcommands]]
name = "add"

[[subcommands.subcommands]]
name = "remove"

[[subcommands.subcommands.args]]
exec = "git remote"
"#;

    #[test]
    fn parse_full_example() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        assert_eq!(spec.args, vec![CandidateSource::Builtin(BuiltinType::File)]);
        assert_eq!(spec.flags.len(), 2);
        assert_eq!(spec.flags[0].names, vec!["-C"]);
        assert_eq!(
            spec.flags[0].value,
            Some(CandidateSource::Builtin(BuiltinType::Directory))
        );
        assert_eq!(spec.flags[1].names, vec!["--no-pager"]);
        assert_eq!(spec.flags[1].value, None);
        assert_eq!(spec.subcommands.len(), 2);
        let checkout = &spec.subcommands[0];
        assert_eq!(checkout.name, "checkout");
        assert_eq!(
            checkout.args,
            vec![CandidateSource::Exec(
                "git branch --format='%(refname:short)'".to_string()
            )]
        );
        let remote = &spec.subcommands[1];
        assert_eq!(remote.subcommands.len(), 2);
        assert_eq!(remote.subcommands[1].name, "remove");
        assert_eq!(
            remote.subcommands[1].args,
            vec![CandidateSource::Exec("git remote".to_string())]
        );
    }

    #[test]
    fn parse_empty_file_is_valid() {
        let spec = CompletionSpec::parse("").unwrap();
        assert!(spec.args.is_empty());
        assert!(spec.flags.is_empty());
        assert!(spec.subcommands.is_empty());
    }

    #[test]
    fn parse_values_source() {
        let spec = CompletionSpec::parse("[[args]]\nvalues = [\"json\", \"yaml\"]\n").unwrap();
        assert_eq!(
            spec.args,
            vec![CandidateSource::Values(vec![
                "json".to_string(),
                "yaml".to_string()
            ])]
        );
    }

    #[test]
    fn parse_rejects_source_with_no_keys() {
        let err = CompletionSpec::parse("[[args]]\n").unwrap_err();
        assert!(err.contains("exactly one"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_source_with_two_keys() {
        let err =
            CompletionSpec::parse("[[args]]\ntype = \"file\"\nvalues = [\"a\"]\n").unwrap_err();
        assert!(err.contains("exactly one"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_unknown_builtin_type() {
        let err = CompletionSpec::parse("[[args]]\ntype = \"folder\"\n").unwrap_err();
        assert!(err.contains("folder"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_duplicate_subcommand_names() {
        let text = "[[subcommands]]\nname = \"a\"\n[[subcommands]]\nname = \"a\"\n";
        let err = CompletionSpec::parse(text).unwrap_err();
        assert!(
            err.contains("duplicate subcommand"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_rejects_duplicate_flag_spelling() {
        let text = "[[flags]]\nnames = [\"-v\"]\n[[flags]]\nnames = [\"-v\", \"--verbose\"]\n";
        let err = CompletionSpec::parse(text).unwrap_err();
        assert!(err.contains("duplicate flag"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_empty_flag_names() {
        let err = CompletionSpec::parse("[[flags]]\nnames = []\n").unwrap_err();
        assert!(err.contains("names"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_unknown_field() {
        // Typos in the file must surface as a parse error (and thus a
        // one-time warning), not silently do nothing.
        let err = CompletionSpec::parse("[[flags]]\nname = [\"-v\"]\n").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn parse_rejects_duplicates_in_nested_level() {
        let text = "\
[[subcommands]]
name = \"remote\"
[[subcommands.subcommands]]
name = \"add\"
[[subcommands.subcommands]]
name = \"add\"
";
        let err = CompletionSpec::parse(text).unwrap_err();
        assert!(
            err.contains("duplicate subcommand"),
            "unexpected error: {err}"
        );
    }

    // ── SpecStore ────────────────────────────────────────────────────

    fn store_with(specs: &[(&str, &str)]) -> (tempfile::TempDir, SpecStore) {
        let tmp = tempfile::TempDir::new().unwrap();
        for (name, text) in specs {
            std::fs::write(tmp.path().join(format!("{name}.toml")), text).unwrap();
        }
        let store = SpecStore::new(tmp.path().to_path_buf());
        (tmp, store)
    }

    #[test]
    fn store_loads_spec_by_command_name() {
        let (_tmp, mut store) = store_with(&[("mytool", "[[args]]\nvalues = [\"a\"]\n")]);
        let spec = store.get("mytool").unwrap();
        assert_eq!(spec.args.len(), 1);
    }

    #[test]
    fn store_resolves_final_path_component() {
        let (_tmp, mut store) = store_with(&[("git", "[[subcommands]]\nname = \"log\"\n")]);
        assert!(store.get("/usr/bin/git").is_some());
    }

    #[test]
    fn store_missing_file_returns_none() {
        let (_tmp, mut store) = store_with(&[]);
        assert!(store.get("nosuch").is_none());
    }

    #[test]
    fn store_caches_first_load() {
        let (tmp, mut store) = store_with(&[("mytool", "[[args]]\nvalues = [\"a\"]\n")]);
        assert!(store.get("mytool").is_some());
        // Overwrite with garbage; the cached parse must still be served.
        std::fs::write(tmp.path().join("mytool.toml"), "not [ valid").unwrap();
        assert!(store.get("mytool").is_some());
    }

    #[test]
    fn store_parse_error_returns_none() {
        let (_tmp, mut store) = store_with(&[("bad", "not [ valid toml")]);
        assert!(store.get("bad").is_none());
        // Negative-cached: repeat lookups stay None.
        assert!(store.get("bad").is_none());
    }

    #[test]
    fn store_empty_command_returns_none() {
        let (_tmp, mut store) = store_with(&[]);
        assert!(store.get("").is_none());
    }

    // ── command_words ────────────────────────────────────────────────

    #[test]
    fn words_simple() {
        // "git checkout ma|" — word_start = 13
        let (cmd, args) = command_words("git checkout ma", 13).unwrap();
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["checkout"]);
    }

    #[test]
    fn words_cursor_on_command_is_none() {
        assert!(command_words("gi", 0).is_none());
        assert!(command_words("", 0).is_none());
    }

    #[test]
    fn words_pipeline_segment_only() {
        // "cat f | git checkout ma|" — word_start = 21
        let (cmd, args) = command_words("cat f | git checkout ma", 21).unwrap();
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["checkout"]);
    }

    #[test]
    fn words_after_semicolon() {
        // "echo a; git lo|" — word_start = 12
        let (cmd, args) = command_words("echo a; git lo", 12).unwrap();
        assert_eq!(cmd, "git");
        assert!(args.is_empty());
    }

    #[test]
    fn words_strips_quotes() {
        // "git commit -m 'a b' |" — word_start = 20
        let (cmd, args) = command_words("git commit -m 'a b' ", 20).unwrap();
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["commit", "-m", "a b"]);
    }

    #[test]
    fn words_skips_bang_prefix() {
        // "! git lo|" — word_start = 6
        let (cmd, args) = command_words("! git lo", 6).unwrap();
        assert_eq!(cmd, "git");
        assert!(args.is_empty());
    }

    // ── resolve ──────────────────────────────────────────────────────

    fn as_strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn resolve_top_level_positional() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, keep) = resolve(&spec, &[], "");
        match res {
            Resolution::Positional {
                subcommands,
                source,
            } => {
                assert_eq!(subcommands, vec!["checkout", "remote"]);
                assert_eq!(source, Some(&CandidateSource::Builtin(BuiltinType::File)));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(keep, "");
    }

    #[test]
    fn resolve_descends_into_subcommand() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["checkout"]), "ma");
        match res {
            Resolution::Positional {
                subcommands,
                source,
            } => {
                assert!(subcommands.is_empty());
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec(
                        "git branch --format='%(refname:short)'".to_string()
                    ))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_nested_subcommand() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["remote", "remove"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec("git remote".to_string()))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_value_flag_completes_next_word() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["-C"]), "");
        match res {
            Resolution::FlagValue(source) => {
                assert_eq!(source, &CandidateSource::Builtin(BuiltinType::Directory));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_value_flag_consumes_its_value() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        // "-C /tmp checkout <cursor>" — /tmp is the value of -C, then descend.
        let (res, _) = resolve(&spec, &as_strings(&["-C", "/tmp", "checkout"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec(
                        "git branch --format='%(refname:short)'".to_string()
                    ))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_boolean_flag_is_consumed() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["--no-pager"]), "");
        assert!(matches!(res, Resolution::Positional { .. }));
    }

    #[test]
    fn resolve_unknown_dash_word_is_consumed_as_boolean() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, _) = resolve(&spec, &as_strings(&["-q", "checkout"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Exec(
                        "git branch --format='%(refname:short)'".to_string()
                    ))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_dash_word_completes_flag_names() {
        let spec = CompletionSpec::parse(GIT_SPEC).unwrap();
        let (res, keep) = resolve(&spec, &[], "--n");
        match res {
            Resolution::FlagNames(names) => {
                assert_eq!(names, vec!["-C", "--no-pager"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(keep, "");
    }

    #[test]
    fn resolve_flag_eq_value_form() {
        let text = "\
[[flags]]
names = [\"--format\"]
value = { values = [\"json\", \"yaml\"] }
";
        let spec = CompletionSpec::parse(text).unwrap();
        let (res, keep) = resolve(&spec, &[], "--format=j");
        match res {
            Resolution::FlagValue(source) => {
                assert_eq!(
                    source,
                    &CandidateSource::Values(vec!["json".to_string(), "yaml".to_string()])
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(keep, "--format=");
    }

    #[test]
    fn resolve_flag_eq_value_in_prior_word_is_self_contained() {
        let text = "\
[[flags]]
names = [\"--format\"]
value = { values = [\"json\"] }
[[args]]
values = [\"target\"]
";
        let spec = CompletionSpec::parse(text).unwrap();
        // "--format=json <cursor>" must NOT treat the cursor word as the
        // flag's value.
        let (res, _) = resolve(&spec, &as_strings(&["--format=json"]), "");
        match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Values(vec!["target".to_string()]))
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resolve_positional_index_and_repeat_last() {
        let text = "\
[[args]]
values = [\"first\"]
[[args]]
values = [\"second\"]
";
        let spec = CompletionSpec::parse(text).unwrap();
        let expect_values = |res: Resolution<'_>, want: &str| match res {
            Resolution::Positional { source, .. } => {
                assert_eq!(
                    source,
                    Some(&CandidateSource::Values(vec![want.to_string()]))
                );
            }
            other => panic!("unexpected: {other:?}"),
        };
        let (res, _) = resolve(&spec, &[], "");
        expect_values(res, "first");
        let (res, _) = resolve(&spec, &as_strings(&["x"]), "");
        expect_values(res, "second");
        // Last entry repeats for further positionals.
        let (res, _) = resolve(&spec, &as_strings(&["x", "y"]), "");
        expect_values(res, "second");
    }

    #[test]
    fn resolve_no_args_declared_has_no_source() {
        let spec = CompletionSpec::parse("[[subcommands]]\nname = \"sub\"\n").unwrap();
        let (res, _) = resolve(&spec, &[], "");
        match res {
            Resolution::Positional {
                subcommands,
                source,
            } => {
                assert_eq!(subcommands, vec!["sub"]);
                assert_eq!(source, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    // ── run_exec ─────────────────────────────────────────────────────

    use std::time::Duration;

    #[test]
    fn exec_splits_stdout_lines_and_drops_empties() {
        let lines = run_exec("printf 'alpha\\n\\nbeta\\n'", Duration::from_secs(5), None);
        assert_eq!(lines, vec!["alpha", "beta"]);
    }

    #[test]
    fn exec_nonzero_exit_yields_no_candidates() {
        let lines = run_exec("echo out; exit 1", Duration::from_secs(5), None);
        assert!(lines.is_empty());
    }

    #[test]
    fn exec_timeout_kills_child_and_yields_no_candidates() {
        let start = std::time::Instant::now();
        let lines = run_exec("sleep 5", Duration::from_millis(100), None);
        assert!(lines.is_empty());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout did not fire: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn exec_inherits_cwd() {
        let _guard = crate::test_sync::lock_cwd();
        // The child runs in the shell's cwd, so `pwd` output is non-empty
        // and matches the current dir.
        let cwd = std::env::current_dir().unwrap();
        let lines = run_exec("pwd", Duration::from_secs(5), None);
        assert_eq!(lines, vec![cwd.to_string_lossy().to_string()]);
    }

    #[test]
    fn exec_backgrounded_grandchild_does_not_hang() {
        let start = std::time::Instant::now();
        let lines = run_exec("sleep 5 & echo hi", Duration::from_millis(100), None);
        assert!(lines.is_empty());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "reader join must be bounded: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn exec_uses_provided_env() {
        let env = vec![
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("YOSH_TEST_MARKER".to_string(), "marker42".to_string()),
        ];
        let lines = run_exec(
            "printf '%s\\n' \"$YOSH_TEST_MARKER\"",
            Duration::from_secs(5),
            Some(&env),
        );
        assert_eq!(lines, vec!["marker42"]);
    }

    // ── try_complete ─────────────────────────────────────────────────

    use crate::env::aliases::AliasStore;
    use crate::interactive::command_completion::{CommandCompleter, CommandCompletionContext};
    use crate::interactive::completion::CompletionContext;

    /// Run try_complete over `line` (cursor at end) against a single spec
    /// named `cmd`, using a scratch cwd.
    fn spec_complete(cmd: &str, spec_text: &str, line: &str, cwd: &str) -> Option<SpecCompletion> {
        let (_tmp, mut store) = store_with(&[(cmd, spec_text)]);
        let ctx = CompletionContext {
            cwd: cwd.to_string(),
            home: "/home/user".to_string(),
            show_dotfiles: false,
        };
        let aliases = AliasStore::default();
        let mut completer = CommandCompleter::new();
        let mut cmd_ctx = CommandCompletionContext {
            completer: &mut completer,
            path: "",
            builtins: &["echo", "exit", "cd"],
            aliases: &aliases,
        };
        let pos = line.len();
        let (word_start, word) = crate::interactive::completion::extract_completion_word(line, pos);
        let word = word.to_string();
        try_complete(line, word_start, &word, &mut store, &ctx, &mut cmd_ctx)
    }

    #[test]
    fn complete_values_filters_by_prefix() {
        let result = spec_complete(
            "mytool",
            "[[args]]\nvalues = [\"alpha\", \"omega\", \"alto\"]\n",
            "mytool al",
            "/",
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["alpha", "alto"]);
        assert_eq!(result.common_prefix, "al");
        assert_eq!(result.keep_prefix, "");
    }

    #[test]
    fn complete_no_spec_returns_none() {
        let result = spec_complete("mytool", "[[args]]\nvalues = [\"a\"]\n", "other x", "/");
        assert!(result.is_none());
    }

    #[test]
    fn complete_none_source_suppresses_fallback() {
        let result =
            spec_complete("mytool", "[[args]]\ntype = \"none\"\n", "mytool x", "/").unwrap();
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn complete_none_source_still_offers_subcommands() {
        let text = "[[args]]\ntype = \"none\"\n[[subcommands]]\nname = \"deploy\"\n";
        let result = spec_complete("mytool", text, "mytool dep", "/").unwrap();
        assert_eq!(result.candidates, vec!["deploy"]);
    }

    #[test]
    fn complete_zero_matches_falls_back() {
        // Candidates exist but none match the word → fallback (None).
        let result = spec_complete(
            "mytool",
            "[[args]]\nvalues = [\"alpha\"]\n",
            "mytool zz",
            "/",
        );
        assert!(result.is_none());
    }

    #[test]
    fn complete_subcommand_names_merge_with_values() {
        let text = "\
[[args]]
values = [\"deep\"]
[[subcommands]]
name = \"deploy\"
";
        let result = spec_complete("mytool", text, "mytool de", "/").unwrap();
        assert_eq!(result.candidates, vec!["deep", "deploy"]);
    }

    #[test]
    fn complete_flag_names() {
        let text = "\
[[flags]]
names = [\"--verbose\", \"-v\"]
[[flags]]
names = [\"--version\"]
";
        let result = spec_complete("mytool", text, "mytool --ver", "/").unwrap();
        assert_eq!(result.candidates, vec!["--verbose", "--version"]);
    }

    #[test]
    fn complete_flag_eq_value_keeps_prefix() {
        let text = "\
[[flags]]
names = [\"--format\"]
value = { values = [\"json\", \"yaml\"] }
";
        let result = spec_complete("mytool", text, "mytool --format=j", "/").unwrap();
        assert_eq!(result.candidates, vec!["json"]);
        assert_eq!(result.keep_prefix, "--format=");
    }

    #[test]
    fn complete_pure_file_source_falls_back_to_path_completion() {
        // type = "file" with no subcommands defers entirely to the
        // caller's existing path completion.
        let result = spec_complete("mytool", "[[args]]\ntype = \"file\"\n", "mytool x", "/");
        assert!(result.is_none());
    }

    #[test]
    fn complete_directory_source_filters_to_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        std::fs::File::create(tmp.path().join("subfile.txt")).unwrap();
        let result = spec_complete(
            "mytool",
            "[[args]]\ntype = \"directory\"\n",
            "mytool su",
            tmp.path().to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["subdir/"]);
    }

    #[test]
    fn complete_flag_eq_directory_value() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        std::fs::File::create(tmp.path().join("subfile.txt")).unwrap();
        let text = "[[flags]]\nnames = [\"--dest\"]\nvalue = { type = \"directory\" }\n";
        let result = spec_complete(
            "mytool",
            text,
            "mytool --dest=su",
            tmp.path().to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["subdir/"]);
        assert_eq!(result.keep_prefix, "--dest=");
    }

    #[test]
    fn complete_flag_eq_file_value_in_subdir() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        std::fs::File::create(tmp.path().join("sub").join("nested.txt")).unwrap();
        let text = "[[flags]]\nnames = [\"--config\"]\nvalue = { type = \"file\" }\n";
        let result = spec_complete(
            "mytool",
            text,
            "mytool --config=sub/ne",
            tmp.path().to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["nested.txt"]);
        assert_eq!(result.keep_prefix, "--config=sub/");
    }

    #[test]
    fn complete_command_source_uses_command_completer() {
        let result =
            spec_complete("mytool", "[[args]]\ntype = \"command\"\n", "mytool e", "/").unwrap();
        // builtins list in the harness: echo, exit (both match "e"), cd.
        assert_eq!(result.candidates, vec!["echo", "exit"]);
    }

    #[test]
    fn complete_exec_source_runs_command() {
        let result = spec_complete(
            "mytool",
            "[[args]]\nexec = \"printf 'alpha\\\\nomega\\\\n'\"\n",
            "mytool al",
            "/",
        )
        .unwrap();
        assert_eq!(result.candidates, vec!["alpha"]);
    }

    #[test]
    fn complete_exec_empty_output_falls_back() {
        let result = spec_complete("mytool", "[[args]]\nexec = \"true\"\n", "mytool x", "/");
        assert!(result.is_none());
    }

    #[test]
    fn complete_cursor_on_command_word_returns_none() {
        let result = spec_complete("mytool", "[[args]]\nvalues = [\"a\"]\n", "mytoo", "/");
        assert!(result.is_none());
    }
}
