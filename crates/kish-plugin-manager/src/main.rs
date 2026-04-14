use std::process;

use clap::{Parser, Subcommand};
use kish_plugin_manager::{config, github, lockfile, sync, verify};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("KISH_GIT_HASH"),
    " ",
    env!("KISH_BUILD_DATE"),
    ")"
);

#[derive(Parser)]
#[command(name = "kish-plugin", about = "Manage kish shell plugins")]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        name: Option<String>,
    },
    /// List installed plugins
    List,
    /// Verify plugin integrity (SHA-256)
    Verify,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Sync { prune } => cmd_sync(prune),
        Commands::Update { name } => cmd_update(name.as_deref()),
        Commands::List => cmd_list(),
        Commands::Verify => cmd_verify(),
    };
    process::exit(code);
}

fn cmd_sync(prune: bool) -> i32 {
    let result = match sync::sync(prune) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
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
            "kish-plugin: sync complete ({} plugins)",
            result.succeeded.len()
        );
        0
    } else {
        eprintln!(
            "kish-plugin: sync partial ({} succeeded, {} failed)",
            result.succeeded.len(),
            result.failed.len()
        );
        1
    }
}

fn cmd_update(name_filter: Option<&str>) -> i32 {
    let config_path = sync::config_path();
    let decls = match config::load_config(&config_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    let client = github::GitHubClient::new();

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kish-plugin: {}: {}", config_path.display(), e);
            return 2;
        }
    };
    let mut new_content = content.clone();
    let mut updated = false;

    for decl in &decls {
        if let Some(filter) = name_filter {
            if decl.name != filter {
                continue;
            }
        }
        if let config::PluginSource::GitHub { owner, repo } = &decl.source {
            match client.latest_version(owner, repo) {
                Ok(latest) => {
                    let current = decl.version.as_deref().unwrap_or("");
                    if latest != current {
                        eprintln!("  {} {} \u{2192} {}", decl.name, current, latest);
                        if !current.is_empty() {
                            new_content = new_content.replacen(
                                &format!("version = \"{}\"", current),
                                &format!("version = \"{}\"", latest),
                                1,
                            );
                        }
                        updated = true;
                    } else {
                        eprintln!("  {} {} (already latest)", decl.name, current);
                    }
                }
                Err(e) => {
                    eprintln!("  \u{2717} {}: {}", decl.name, e);
                }
            }
        }
    }

    if updated {
        if let Err(e) = std::fs::write(&config_path, &new_content) {
            eprintln!("kish-plugin: write {}: {}", config_path.display(), e);
            return 2;
        }
        return cmd_sync(false);
    }

    0
}

fn cmd_list() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
            return 2;
        }
    };

    if lockfile.plugin.is_empty() {
        eprintln!("no plugins installed (run 'kish-plugin sync' first)");
        return 0;
    }

    for entry in &lockfile.plugin {
        let version = entry.version.as_deref().unwrap_or("-");
        let verified = match verify::verify_checksum(
            &config::expand_tilde_path(&entry.path),
            &entry.sha256,
        ) {
            Ok(true) => "\u{2713} verified",
            Ok(false) => "\u{2717} checksum mismatch",
            Err(_) => "\u{2717} file missing",
        };
        println!(
            "{:<16} {:<8} {:<48} {}",
            entry.name, version, entry.source, verified
        );
    }

    0
}

fn cmd_verify() -> i32 {
    let lock_path = sync::lock_path();
    let lockfile = match lockfile::load_lockfile(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kish-plugin: {}", e);
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
