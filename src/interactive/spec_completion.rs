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
}

impl SpecStore {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            cache: std::collections::HashMap::new(),
        }
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
}
