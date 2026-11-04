//! [`SpecStore`]: lazy per-command loading of completion specs.

use super::{CompletionSpec, EMBEDDED_SPECS};

/// Lazy loader and per-session cache for completion specs.
///
/// Specs live at `<dir>/<command>.toml`. Each command name is looked up
/// on disk at most once per session. For a plain store (`new`), a
/// missing file and a parse failure both cache as `None`; for an
/// embedded-fallback store (`with_embedded`/`from_home`), a missing file
/// resolves to the embedded spec (if any) while a parse failure still
/// caches as `None` without falling back. Caching failures as `None`
/// also makes the parse-error warning naturally print only once.
pub struct SpecStore {
    dir: std::path::PathBuf,
    cache: std::collections::HashMap<String, Option<CompletionSpec>>,
    pub(super) exec_env: Option<Vec<(String, String)>>,
    use_embedded: bool,
}

impl SpecStore {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            cache: std::collections::HashMap::new(),
            exec_env: None,
            use_embedded: false,
        }
    }

    /// Store that serves [`EMBEDDED_SPECS`] for commands with no spec
    /// file in `dir`. A file in `dir` always wins over the embedded
    /// spec, including a file that fails to parse.
    pub fn with_embedded(dir: std::path::PathBuf) -> Self {
        Self {
            use_embedded: true,
            ..Self::new(dir)
        }
    }

    /// Per-prompt snapshot of the shell's exported variables, passed to
    /// `exec` candidate commands. `None` inherits the process env.
    pub fn set_exec_env(&mut self, env: Vec<(String, String)>) {
        self.exec_env = Some(env);
    }

    /// Store rooted at the standard location under `home`
    /// (`~/.config/yosh/completions`), with embedded-spec fallback.
    pub fn from_home(home: &str) -> Self {
        Self::with_embedded(std::path::PathBuf::from(home).join(".config/yosh/completions"))
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
        match std::fs::read_to_string(&path) {
            Ok(text) => match CompletionSpec::parse(&text) {
                Ok(spec) => Some(spec),
                // A broken user file must not silently fall back to the
                // embedded spec — the warning tells the user which file
                // to fix, and stale defaults would mask their edits.
                Err(err) => {
                    eprintln!("yosh: completion: {name}.toml: {err}");
                    None
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => self.load_embedded(name),
            Err(err) => {
                eprintln!("yosh: completion: {name}.toml: {err}");
                None
            }
        }
    }

    fn load_embedded(&self, name: &str) -> Option<CompletionSpec> {
        if !self.use_embedded {
            return None;
        }
        let (_, text) = EMBEDDED_SPECS.iter().find(|(n, _)| *n == name)?;
        // Embedded specs are validated by unit tests; a parse failure
        // here means a build-toolchain bug, not a user error.
        CompletionSpec::parse(text).ok()
    }
}
