use std::process;

mod config;
mod github;
mod lockfile;
mod resolve;
mod sync;
mod verify;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("sync") => cmd_sync(&args[1..]),
        Some("update") => cmd_update(&args[1..]),
        Some("list") => cmd_list(),
        Some("verify") => cmd_verify(),
        Some(cmd) => {
            eprintln!("kish-plugin: unknown command '{}'", cmd);
            2
        }
        None => {
            eprintln!("usage: kish-plugin <sync|update|list|verify>");
            2
        }
    };
    process::exit(code);
}

fn cmd_sync(args: &[String]) -> i32 {
    let prune = args.iter().any(|a| a == "--prune");
    let result = sync::sync(prune);

    for name in &result.succeeded {
        eprintln!("  ✓ {}", name);
    }
    for (name, err) in &result.failed {
        eprintln!("  ✗ {}: {}", name, err);
    }

    if result.failed.is_empty() {
        eprintln!("kish-plugin: sync complete ({} plugins)", result.succeeded.len());
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

fn cmd_update(args: &[String]) -> i32 {
    let name_filter = args.first().map(|s| s.as_str());

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
                        eprintln!("  {} {} → {}", decl.name, current, latest);
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
                    eprintln!("  ✗ {}: {}", decl.name, e);
                }
            }
        }
    }

    if updated {
        if let Err(e) = std::fs::write(&config_path, &new_content) {
            eprintln!("kish-plugin: write {}: {}", config_path.display(), e);
            return 2;
        }
        return cmd_sync(&[]);
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
            Ok(true) => "✓ verified",
            Ok(false) => "✗ checksum mismatch",
            Err(_) => "✗ file missing",
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
                eprintln!("  ✓ {}", entry.name);
            }
            Ok(false) => {
                eprintln!("  ✗ {}: checksum mismatch", entry.name);
                all_ok = false;
            }
            Err(e) => {
                eprintln!("  ✗ {}: {}", entry.name, e);
                all_ok = false;
            }
        }
    }

    if all_ok { 0 } else { 1 }
}
