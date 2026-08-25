pub mod config;
pub mod github;
pub mod install;
pub mod lockfile;
pub mod metadata_extract;
pub mod precompile;
pub mod resolve;
pub mod runner;
pub mod scenario;
pub mod sync;
pub mod test_host;
pub(crate) mod tick;
pub(crate) mod trace;
pub mod update;
pub mod verify;
pub(crate) mod watch;

/// wasmtime bindgen for the `plugin-world` WIT contract.
///
/// Path is `wit/` inside this crate. The canonical source lives in
/// `yosh-plugin-api/wit/`; `build.rs` verifies the bundled copy matches
/// when built inside the workspace. The copy is required because
/// `cargo install yosh-plugin-manager` extracts each crate standalone,
/// so a sibling-relative path (`../yosh-plugin-api/wit`) is unresolvable
/// from `~/.cargo/registry/src/.../yosh-plugin-manager-<ver>/`.
///
/// This is independent from the host's bindgen invocation in
/// `src/plugin/mod.rs` — the two crates produce separate generated
/// types, so we cannot share. The host needs `HostContext` as the store
/// type and full host imports; the manager needs `MetadataCtx` and
/// deny-only imports.
pub mod generated {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "plugin-world",
        async: false,
    });
}

use clap::{Parser, Subcommand};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("YOSH_GIT_HASH"),
    " ",
    env!("YOSH_BUILD_DATE"),
    ")"
);

#[derive(Parser)]
#[command(name = "yosh-plugin", about = "Manage yosh shell plugins")]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum RunAction {
    /// Call `plugin/exec` with the given command and argv.
    Exec { command: String, args: Vec<String> },
    /// Call one hook.
    Hook {
        #[command(subcommand)]
        which: HookKind,
    },
}

#[derive(Subcommand)]
pub(crate) enum HookKind {
    PreExec {
        command_line: String,
    },
    PostExec {
        command_line: String,
        exit_code: i32,
    },
    OnCd {
        old: String,
        new: String,
    },
    PrePrompt,
}

#[derive(Copy, Clone, clap::ValueEnum, Debug)]
pub(crate) enum OutputFormat {
    Human,
    Json,
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got `{}`", s))?;
    Ok((k.to_string(), v.to_string()))
}

#[derive(Subcommand)]
enum Commands {
    /// Install plugins from plugins.toml
    Sync {
        /// Remove plugins not in plugins.toml
        #[arg(long)]
        prune: bool,
    },
    /// Update installed plugins to latest version
    Update {
        /// Only update the named plugin
        #[arg(value_name = "PLUGIN")]
        name: Option<String>,
    },
    /// List installed plugins
    List,
    /// Verify plugin integrity (SHA-256)
    Verify,
    /// Add a plugin from a GitHub URL or local path to plugins.toml
    Install {
        /// GitHub URL (https://github.com/owner/repo[@version]) or local file path
        source: String,
        /// Overwrite existing plugin with the same name
        #[arg(long)]
        force: bool,
    },
    /// Run a single exec / hook against a plugin wasm with an in-memory host.
    Run {
        /// Path to the wasm component.
        wasm: std::path::PathBuf,
        #[command(subcommand)]
        action: RunAction,
        /// Capabilities to grant (comma-separated, e.g. `io,variables:read`).
        /// Defaults to the plugin's declared `required_capabilities`.
        #[arg(long, value_delimiter = ',')]
        cap: Vec<String>,
        /// Seed a shell variable: `--var KEY=VALUE` (repeatable).
        #[arg(long = "var", value_parser = parse_kv)]
        vars: Vec<(String, String)>,
        /// Seed an exported variable.
        #[arg(long = "export", value_parser = parse_kv)]
        exports: Vec<(String, String)>,
        /// Virtual cwd.
        #[arg(long, default_value = ".")]
        cwd: std::path::PathBuf,
        /// Allowlist pattern for `commands:exec` (repeatable).
        #[arg(long = "allow-exec")]
        allow_exec: Vec<String>,
        /// If set, files:* operate on the real FS scoped here.
        #[arg(long = "sandbox-root")]
        sandbox_root: Option<std::path::PathBuf>,
        /// Watchdog deadline in milliseconds.
        #[arg(long, default_value_t = 5000)]
        timeout: u64,
        /// Linear-memory cap for the plugin store, in MiB.
        #[arg(long = "max-memory-mb", default_value_t = 256)]
        max_memory_mb: u64,
        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
        /// Re-run the invocation whenever the wasm file changes
        /// (mtime-polled every 300 ms). Ctrl-C to stop.
        #[arg(long)]
        watch: bool,
    },
    /// Run declarative scenarios (TOML) from a directory.
    Test {
        /// Directory or single file. Default: `tests/`.
        #[arg(default_value = "tests")]
        path: std::path::PathBuf,
        /// Regex filter over the scenario file path.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    match cli.command {
        Commands::Sync { prune } => cmd_sync(prune),
        Commands::Update { name } => cmd_update(name.as_deref()),
        Commands::List => cmd_list(),
        Commands::Verify => cmd_verify(),
        Commands::Install { source, force } => cmd_install(&source, force),
        Commands::Run {
            wasm,
            action,
            cap,
            vars,
            exports,
            cwd,
            allow_exec,
            sandbox_root,
            timeout,
            max_memory_mb,
            format,
            watch,
        } => cmd_run(
            wasm,
            action,
            cap,
            vars,
            exports,
            cwd,
            allow_exec,
            sandbox_root,
            timeout,
            max_memory_mb,
            format,
            watch,
        ),
        Commands::Test {
            path,
            filter,
            format,
        } => cmd_test(path, filter, format),
    }
}

fn cmd_test(path: std::path::PathBuf, filter: Option<String>, format: OutputFormat) -> i32 {
    let reports = crate::scenario::run_dir(&path, filter.as_deref());
    // Zero scenarios means the path is wrong (or the filter matched
    // nothing) — "0 passed, 0 failed" exiting 0 would let a typo'd CI
    // path go green with no tests run.
    if reports.is_empty() {
        eprintln!(
            "yosh-plugin: no scenario .toml files found under {}",
            path.display()
        );
        return 1;
    }
    let all_passed = reports.iter().all(|r| r.passed());
    match format {
        OutputFormat::Human => print!("{}", crate::scenario::format_summary_human(&reports)),
        OutputFormat::Json => print!("{}", crate::scenario::format_summary_json(&reports)),
    }
    if all_passed { 0 } else { 1 }
}

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    wasm: std::path::PathBuf,
    action: RunAction,
    cap: Vec<String>,
    vars: Vec<(String, String)>,
    exports: Vec<(String, String)>,
    cwd: std::path::PathBuf,
    allow_exec: Vec<String>,
    sandbox_root: Option<std::path::PathBuf>,
    timeout: u64,
    max_memory_mb: u64,
    format: OutputFormat,
    watch: bool,
) -> i32 {
    if !watch {
        return match run_once(
            &wasm,
            &action,
            &cap,
            &vars,
            &exports,
            &cwd,
            &allow_exec,
            sandbox_root.as_deref(),
            timeout,
            max_memory_mb,
            format,
        ) {
            Ok(code) => code,
            Err(e) => {
                emit_harness_error(&e, format);
                99
            }
        };
    }
    // --watch: re-run on every wasm mtime change until Ctrl-C (default
    // SIGINT disposition kills the process — no handler needed). Errors
    // don't end the loop: a broken build prints its error, then the
    // next successful build re-runs.
    //
    // Watching is pointless if the wasm can't be read at startup (spec
    // §3.6): run once and exit like non-watch mode. Once watching has
    // started, later errors (e.g. a broken rebuild) keep the loop alive.
    let Some(initial_mtime) = std::fs::metadata(&wasm).and_then(|m| m.modified()).ok() else {
        return match run_once(
            &wasm,
            &action,
            &cap,
            &vars,
            &exports,
            &cwd,
            &allow_exec,
            sandbox_root.as_deref(),
            timeout,
            max_memory_mb,
            format,
        ) {
            Ok(code) => code,
            Err(e) => {
                emit_harness_error(&e, format);
                99
            }
        };
    };
    let mut last = Some(initial_mtime);
    loop {
        match run_once(
            &wasm,
            &action,
            &cap,
            &vars,
            &exports,
            &cwd,
            &allow_exec,
            sandbox_root.as_deref(),
            timeout,
            max_memory_mb,
            format,
        ) {
            Ok(_) => {}
            Err(e) => emit_harness_error(&e, format),
        }
        if matches!(format, OutputFormat::Human) {
            eprintln!("--- watching {} (Ctrl-C to stop) ---", wasm.display());
        }
        last = Some(crate::watch::wait_for_change(&wasm, last));
        if matches!(format, OutputFormat::Human) {
            eprintln!("--- change detected, re-running ---");
        }
    }
}

/// Print a harness-level error: always the human line (+ hint) on
/// stderr; in JSON mode additionally a parseable `{"error":{...}}`
/// object on stdout so `--format json` consumers never have to scrape
/// stderr (spec §3.1).
fn emit_harness_error(e: &crate::runner::HarnessError, format: OutputFormat) {
    eprintln!("yosh-plugin: {}", e);
    if let Some(h) = &e.hint {
        eprintln!("yosh-plugin: hint: {}", h);
    }
    if matches!(format, OutputFormat::Json) {
        println!("{}", e.to_json());
    }
}

/// One complete `run` invocation: read + compile (once) + optional
/// metadata caps fallback + instantiate + invoke + print. Returns the
/// process exit code; every harness-level failure funnels to the
/// caller as `HarnessError`. Extracted so `--watch` (Task 3) can
/// re-run the same body.
#[allow(clippy::too_many_arguments)]
fn run_once(
    wasm: &std::path::Path,
    action: &RunAction,
    cap: &[String],
    vars: &[(String, String)],
    exports: &[(String, String)],
    cwd: &std::path::Path,
    allow_exec: &[String],
    sandbox_root: Option<&std::path::Path>,
    timeout: u64,
    max_memory_mb: u64,
    format: OutputFormat,
) -> Result<i32, crate::runner::HarnessError> {
    use crate::runner::{
        HarnessError, HookCall, format_human, format_json, invoke_exec, invoke_hook,
        load_plugin_precompiled,
    };
    use crate::test_host::TestState;
    use wasmtime::component::Component;
    use yosh_plugin_api::pattern::CommandPattern;
    use yosh_plugin_api::{capabilities_to_bitflags, parse_capability};

    // Read + compile exactly once; the metadata fallback and
    // instantiation share the artifacts (was: 2x read + 2x compile).
    let bytes = std::fs::read(wasm)
        .map_err(|e| HarnessError::load(format!("read {}: {}", wasm.display(), e)))?;
    crate::trace::trace!("read {} ({} bytes)", wasm.display(), bytes.len());
    let engine = crate::precompile::make_engine()
        .map_err(|e| HarnessError::load(format!("engine: {}", e)))?;
    let component = Component::new(&engine, &bytes)
        .map_err(|e| HarnessError::load(format!("compile: {}", e)))?;
    crate::trace::trace!("compiled component");

    let mut state = TestState::default();
    let parsed_caps: Vec<_> = cap.iter().filter_map(|s| parse_capability(s)).collect();
    state.caps = if cap.is_empty() {
        let m = crate::metadata_extract::extract_component(&engine, &component)
            .map_err(HarnessError::metadata)?;
        let caps: Vec<_> = m
            .required_capabilities
            .iter()
            .filter_map(|s| parse_capability(s))
            .collect();
        capabilities_to_bitflags(&caps)
    } else {
        capabilities_to_bitflags(&parsed_caps)
    };

    for (k, v) in vars {
        state.vars.insert(k.clone(), v.clone());
    }
    for (k, v) in exports {
        state.vars.insert(k.clone(), v.clone());
        state.exported.insert(k.clone());
    }
    state.cwd = cwd.to_path_buf();
    state.allow_exec = allow_exec
        .iter()
        .filter_map(|p| match CommandPattern::parse(p) {
            Ok(pat) => Some(pat),
            Err(e) => {
                eprintln!(
                    "yosh-plugin: ignoring invalid --allow-exec pattern {:?}: {}",
                    p, e
                );
                None
            }
        })
        .collect();
    state.sandbox_root =
        sandbox_root.map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
    state.max_memory_mb = Some(max_memory_mb);

    let loaded = load_plugin_precompiled(
        &engine,
        &component,
        state,
        std::time::Duration::from_millis(timeout),
    )?;

    let outcome = match action {
        RunAction::Exec { command, args } => invoke_exec(loaded, command, args),
        RunAction::Hook { which } => {
            let call = match which {
                HookKind::PreExec { command_line } => HookCall::PreExec {
                    command_line: command_line.clone(),
                },
                HookKind::PostExec {
                    command_line,
                    exit_code,
                } => HookCall::PostExec {
                    command_line: command_line.clone(),
                    exit_code: *exit_code,
                },
                HookKind::OnCd { old, new } => HookCall::OnCd {
                    old: old.clone(),
                    new: new.clone(),
                },
                HookKind::PrePrompt => HookCall::PrePrompt,
            };
            invoke_hook(loaded, call)
        }
    };

    match format {
        OutputFormat::Human => print!("{}", format_human(&outcome)),
        OutputFormat::Json => println!("{}", format_json(&outcome)),
    }

    Ok(match outcome.error_kind {
        Some(_) => 99,
        None => outcome.exit_code.unwrap_or(0),
    })
}

fn cmd_install(source: &str, force: bool) -> i32 {
    let config_path = sync::config_path();
    match install::install(source, force, &config_path, None) {
        Ok(msg) => {
            eprintln!("{}", msg);
            if source.starts_with("https://github.com/") {
                eprintln!("Run 'yosh plugin sync' to download.");
            }
            0
        }
        Err(e) => {
            eprintln!("yosh-plugin: {}", e);
            1
        }
    }
}

fn cmd_sync(prune: bool) -> i32 {
    let result = match sync::sync(prune) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("yosh-plugin: {}", e);
            return 2;
        }
    };

    for name in &result.succeeded {
        eprintln!("  \u{2713} {}", name);
    }
    for (name, err) in &result.failed {
        eprintln!("  \u{2717} {}: {}", name, err);
    }

    if result.failed.is_empty() {
        eprintln!(
            "yosh-plugin: sync complete ({} plugins)",
            result.succeeded.len()
        );
        0
    } else {
        eprintln!(
            "yosh-plugin: sync partial ({} succeeded, {} failed)",
            result.succeeded.len(),
            result.failed.len()
        );
        1
    }
}

fn cmd_update(name_filter: Option<&str>) -> i32 {
    let config_path = sync::config_path();
    let client = github::GitHubClient::new();
    let outcome = match update::update(&config_path, name_filter, &client) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("yosh-plugin: {}", e);
            return 2;
        }
    };

    for result in &outcome.results {
        match &result.status {
            update::UpdateStatus::Updated { from, to } => {
                eprintln!("  {} {} \u{2192} {}", result.name, from, to);
            }
            update::UpdateStatus::AlreadyLatest { current } => {
                eprintln!("  {} {} (already latest)", result.name, current);
            }
            update::UpdateStatus::Failed(e) => {
                eprintln!("  \u{2717} {}: {}", result.name, e);
            }
            update::UpdateStatus::Skipped(_) => {
                // Silent: matches HEAD's behavior of not surfacing
                // name_filter mismatches or local-source skips.
            }
        }
    }

    if outcome.any_updated {
        return cmd_sync(false);
    }

    0
}

fn cmd_list() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("yosh-plugin: {}", e);
            return 2;
        }
    };

    if lockfile.plugin.is_empty() {
        eprintln!("no plugins installed (run 'yosh-plugin sync' first)");
        return 0;
    }

    for entry in &lockfile.plugin {
        let version = entry.version.as_deref().unwrap_or("-");
        let verified =
            match verify::verify_checksum(&config::expand_tilde_path(&entry.path), &entry.sha256) {
                Ok(true) => "\u{2713} verified",
                Ok(false) => "\u{2717} checksum mismatch",
                Err(_) => "\u{2717} file missing",
            };
        // "cached" reflects whether a precompiled cwasm is present AND
        // matches the manager's pinned wasmtime version. A mismatched
        // version means the host will fall back to in-memory precompile
        // at startup — not a hard failure, but worth surfacing here so
        // the user can re-sync.
        let cached = match (&entry.cwasm_path, &entry.wasmtime_version) {
            (Some(p), Some(wv))
                if std::path::Path::new(&config::expand_tilde_path(p)).exists()
                    && wv == precompile::WASMTIME_VERSION =>
            {
                "\u{2713} cached"
            }
            _ => "\u{2717} stale",
        };
        let caps = entry
            .required_capabilities
            .as_ref()
            .map(|v| {
                if v.is_empty() {
                    "[- (no capabilities)]".to_string()
                } else {
                    format!("[{}]", v.join(", "))
                }
            })
            .unwrap_or_else(|| "[?]".into());
        println!(
            "{:<16} {:<8} {:<48} {} {} {}",
            entry.name, version, entry.source, verified, cached, caps
        );
    }

    0
}

fn cmd_verify() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("yosh-plugin: {}", e);
            return 2;
        }
    };

    let mut all_ok = true;
    for entry in &lockfile.plugin {
        let path = config::expand_tilde_path(&entry.path);
        match verify::verify_checksum(&path, &entry.sha256) {
            Ok(true) => {
                eprintln!("  \u{2713} {}", entry.name);
            }
            Ok(false) => {
                eprintln!("  \u{2717} {}: checksum mismatch", entry.name);
                all_ok = false;
            }
            Err(e) => {
                eprintln!("  \u{2717} {}: {}", entry.name, e);
                all_ok = false;
            }
        }
    }

    if all_ok { 0 } else { 1 }
}

#[cfg(test)]
mod cmd_tests {
    use super::*;

    /// Regression: `yosh-plugin test <nonexistent-path>` used to report
    /// "0 passed, 0 failed" and exit 0 — a typo'd path in CI went green
    /// with no tests run.
    #[test]
    fn cmd_test_nonexistent_path_fails() {
        let code = cmd_test(
            std::path::PathBuf::from("/nonexistent/scenario-dir"),
            None,
            OutputFormat::Human,
        );
        assert_eq!(code, 1, "zero scenarios must not exit 0");
    }

    #[test]
    fn cmd_test_empty_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        let code = cmd_test(dir.path().to_path_buf(), None, OutputFormat::Human);
        assert_eq!(
            code, 1,
            "a directory with no .toml scenarios must not exit 0"
        );
    }
}
