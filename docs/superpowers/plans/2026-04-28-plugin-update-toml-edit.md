# `yosh-plugin update` toml_edit Migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `String::replacen` version-rewrite in `cmd_update` with a structural `toml_edit::DocumentMut` rewrite that addresses plugins by `name`, eliminating the silent-corruption bug when two plugins share the same `version` literal.

**Architecture:** Extract orchestration logic from `lib.rs::cmd_update` into a new `src/update.rs` module mirroring the existing `install.rs`/`sync.rs`/`verify.rs` convention. The module exposes a pure `set_plugin_version(&mut DocumentMut, name, new_version)` helper and an `update(config_path, name_filter, client) -> Result<UpdateOutcome, String>` entry point. `cmd_update` becomes a thin printer.

**Tech Stack:** Rust 2024 edition · `toml_edit` 0.22 (already a dependency) · `mockito` 1 (already a dev-dependency) · `tempfile` 3.

**Spec:** `docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `crates/yosh-plugin-manager/src/update.rs` | **Create** | Owns `UpdateStatus` / `UpdateOutcome` types, the `set_plugin_version` TOML helper, and the `update()` orchestrator. ~180 lines including ~11 tests. |
| `crates/yosh-plugin-manager/src/lib.rs` | Modify | Add `pub mod update;`. Replace `cmd_update`'s body with a printer that delegates to `update::update`. |
| `crates/yosh-plugin-manager/src/github.rs` | Modify | Add `#[cfg(test)] pub fn into_client(self) -> GitHubClient` to `GitHubClientWithBase` so integration tests in `update.rs::tests` can pass a mockito-backed `GitHubClient` to `update()`. |
| `TODO.md` | Modify (final task) | Delete the completed `yosh-plugin update: version replacement uses naive String::replacen…` line per CLAUDE.md convention. |

---

## Task 1: Create `update.rs` skeleton with types and module wiring

**Files:**
- Create: `crates/yosh-plugin-manager/src/update.rs`
- Modify: `crates/yosh-plugin-manager/src/lib.rs:1-9`

- [ ] **Step 1: Create `crates/yosh-plugin-manager/src/update.rs` with types and stubs only**

```rust
//! `yosh-plugin update`: structural TOML rewrite of `[[plugin]].version`
//! by plugin `name`, replacing the legacy `String::replacen` flow.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md`.

use std::path::Path;

use toml_edit::DocumentMut;

use crate::config;
use crate::github::GitHubClient;

/// Result of trying to update a single plugin.
#[derive(Debug)]
pub enum UpdateStatus {
    /// Latest differs from current; manifest was rewritten in-memory.
    Updated { from: String, to: String },
    /// Current already matches latest; no rewrite.
    AlreadyLatest { current: String },
    /// Per-plugin GitHub or TOML helper error; loop continues.
    Failed(String),
    /// Plugin was not considered for update for one of the SkipReason variants.
    Skipped(SkipReason),
}

#[derive(Debug)]
pub enum SkipReason {
    /// `name_filter` was Some(X) and this plugin's name was not X.
    NotMatched,
    /// Plugin source is `local:`, not GitHub.
    LocalSource,
    /// Defensive: GitHub plugin has empty/missing `version` field.
    /// `config::load_config` rejects this case, so it should be unreachable
    /// in practice; kept so the loop can surface it cleanly if it ever fires.
    NoCurrentVersion,
}

#[derive(Debug)]
pub struct PluginUpdateResult {
    pub name: String,
    pub status: UpdateStatus,
}

#[derive(Debug)]
pub struct UpdateOutcome {
    pub results: Vec<PluginUpdateResult>,
    /// True iff at least one `UpdateStatus::Updated` was produced.
    /// `cmd_update` reads this to decide whether to invoke `cmd_sync(false)`.
    pub any_updated: bool,
}

/// Orchestration entry point. Reads `config_path`, fetches the latest
/// version of each GitHub plugin (filtered by `name_filter` if set),
/// rewrites matching `[[plugin]].version` fields in a single
/// `DocumentMut`, and writes the result back exactly once if anything
/// changed.
pub fn update(
    _config_path: &Path,
    _name_filter: Option<&str>,
    _client: &GitHubClient,
) -> Result<UpdateOutcome, String> {
    unimplemented!("Task 5")
}

/// Pure TOML helper: locate the `[[plugin]]` table whose `name` equals
/// `name`, then set its `version` field to `new_version`. Returns `Err`
/// on missing/duplicate match or on structural anomalies in the
/// `plugin` key.
pub fn set_plugin_version(
    _doc: &mut DocumentMut,
    _name: &str,
    _new_version: &str,
) -> Result<(), String> {
    unimplemented!("Task 2")
}
```

- [ ] **Step 2: Wire the module into the crate**

Edit `crates/yosh-plugin-manager/src/lib.rs:1-9`. Add `pub mod update;` after `pub mod sync;` so the module list reads:

```rust
pub mod config;
pub mod github;
pub mod install;
pub mod lockfile;
pub mod metadata_extract;
pub mod precompile;
pub mod resolve;
pub mod sync;
pub mod update;
pub mod verify;
```

- [ ] **Step 3: Verify the crate compiles**

Run: `cargo build -p yosh-plugin-manager`
Expected: builds cleanly. `unused` warnings on the new public types are acceptable at this stage.

- [ ] **Step 4: Commit**

```bash
git add crates/yosh-plugin-manager/src/update.rs crates/yosh-plugin-manager/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(plugin-manager): add update.rs skeleton with public types

Skeleton only — set_plugin_version and update return unimplemented!().
Subsequent tasks add tests and implementations.

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい" (selected the version-replacen correctness bug).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `set_plugin_version` — basic in-place replacement (TDD: test #1)

**Files:**
- Modify: `crates/yosh-plugin-manager/src/update.rs`

- [ ] **Step 1: Append a `#[cfg(test)] mod tests` block with test #1**

Add to the bottom of `crates/yosh-plugin-manager/src/update.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_version_basic_replaces_existing() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "foo", "2.0.0").unwrap();
        let out = doc.to_string();
        assert!(out.contains(r#"version = "2.0.0""#), "out:\n{}", out);
        assert!(!out.contains(r#"version = "1.0.0""#), "out:\n{}", out);
    }
}
```

- [ ] **Step 2: Run the test to confirm it fails on `unimplemented!()`**

Run: `cargo test -p yosh-plugin-manager update::tests::set_version_basic_replaces_existing -- --nocapture`
Expected: `FAILED` with panic message containing `not implemented: Task 2` (or `not yet implemented`).

- [ ] **Step 3: Replace the `set_plugin_version` stub with a working implementation**

Replace the body of `set_plugin_version` in `crates/yosh-plugin-manager/src/update.rs`:

```rust
pub fn set_plugin_version(
    doc: &mut DocumentMut,
    name: &str,
    new_version: &str,
) -> Result<(), String> {
    let plugin_item = doc
        .get_mut("plugin")
        .ok_or_else(|| format!("plugin '{}' not found in config", name))?;
    let plugins = plugin_item
        .as_array_of_tables_mut()
        .ok_or_else(|| "config 'plugin' key is not an array of tables".to_string())?;

    let matches: Vec<usize> = plugins
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            if t.get("name").and_then(|v| v.as_str()) == Some(name) {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    match matches.as_slice() {
        [] => Err(format!("plugin '{}' not found in config", name)),
        [idx] => {
            plugins
                .get_mut(*idx)
                .expect("index from filter_map is in-bounds")
                .insert("version", toml_edit::value(new_version));
            Ok(())
        }
        _ => Err(format!(
            "plugin '{}' appears multiple times in config",
            name
        )),
    }
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test -p yosh-plugin-manager update::tests::set_version_basic_replaces_existing`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/yosh-plugin-manager/src/update.rs
git commit -m "$(cat <<'EOF'
feat(plugin-manager): set_plugin_version structural TOML rewrite

Implements the bug-fix core: locate [[plugin]] tables by name (not by
version literal) and rewrite version in-place via toml_edit. Errors on
no-match, multi-match, and structural anomalies.

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Regression coverage — siblings, layout, missing-version, unknown-name

**Files:**
- Modify: `crates/yosh-plugin-manager/src/update.rs`

These four tests should already pass against the Task 2 implementation (siblings: matched by name; layout: toml_edit preserves trivia; missing version: `Table::insert` appends; unknown name: empty match → `Err`). They are added as explicit regression coverage.

- [ ] **Step 1: Add tests #2, #3, #4, #5 to the `mod tests` block**

Append inside `mod tests { ... }` in `crates/yosh-plugin-manager/src/update.rs`:

```rust
    #[test]
    fn set_version_same_version_siblings_no_collision() {
        let toml = r#"[[plugin]]
name = "alpha"
source = "github:owner/alpha"
version = "1.0.0"
enabled = true

[[plugin]]
name = "beta"
source = "github:owner/beta"
version = "1.0.0"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "beta", "1.1.0").unwrap();
        let out = doc.to_string();

        let reparsed = out.parse::<DocumentMut>().unwrap();
        let plugins = reparsed["plugin"].as_array_of_tables().unwrap();
        assert_eq!(plugins.len(), 2);

        let alpha = plugins
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("alpha"))
            .expect("alpha entry survives");
        let beta = plugins
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("beta"))
            .expect("beta entry survives");

        assert_eq!(
            alpha.get("version").and_then(|v| v.as_str()),
            Some("1.0.0"),
            "sibling alpha was modified"
        );
        assert_eq!(
            beta.get("version").and_then(|v| v.as_str()),
            Some("1.1.0"),
            "target beta was not updated"
        );
    }

    #[test]
    fn set_version_preserves_comments_and_layout() {
        let toml = r#"# yosh plugin manifest
# managed by yosh-plugin

[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "foo", "1.1.0").unwrap();
        let out = doc.to_string();
        assert!(out.contains("# yosh plugin manifest"), "out:\n{}", out);
        assert!(out.contains("# managed by yosh-plugin"), "out:\n{}", out);
        assert!(out.contains(r#"version = "1.1.0""#), "out:\n{}", out);
    }

    #[test]
    fn set_version_inserts_when_missing() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
enabled = true
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        set_plugin_version(&mut doc, "foo", "1.0.0").unwrap();
        let out = doc.to_string();
        assert!(out.contains(r#"version = "1.0.0""#), "out:\n{}", out);
    }

    #[test]
    fn set_version_unknown_name_errors() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "nonexistent", "2.0.0").unwrap_err();
        assert!(err.contains("nonexistent"), "err: {}", err);
        assert!(err.contains("not found"), "err: {}", err);
    }
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test -p yosh-plugin-manager update::tests::set_version_`
Expected: 5 tests pass (the original test #1 plus the four new ones).

- [ ] **Step 3: Commit**

```bash
git add crates/yosh-plugin-manager/src/update.rs
git commit -m "$(cat <<'EOF'
test(plugin-manager): regression coverage for set_plugin_version

Adds the same-version-siblings test (the bug-fix proof), layout
preservation, missing-field insertion, and unknown-name error paths.

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Defensive errors — no-array, wrong-type, duplicate-name (TDD: tests #6, #7, #7b)

**Files:**
- Modify: `crates/yosh-plugin-manager/src/update.rs`

The Task 2 implementation already covers tests #6 and #7 (the `get_mut("plugin")` and `as_array_of_tables_mut` paths) and test #7b (the multi-match arm). These tests are added as locked-in coverage.

- [ ] **Step 1: Add tests #6, #7, #7b to the `mod tests` block**

Append inside `mod tests { ... }` in `crates/yosh-plugin-manager/src/update.rs`:

```rust
    #[test]
    fn set_version_no_plugin_array_errors() {
        let toml = "# empty config\n";
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "foo", "1.0.0").unwrap_err();
        assert!(err.contains("not found"), "err: {}", err);
    }

    #[test]
    fn set_version_plugin_key_wrong_type_errors() {
        let toml = "plugin = \"not-an-array\"\n";
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "foo", "1.0.0").unwrap_err();
        assert!(err.contains("array of tables"), "err: {}", err);
    }

    #[test]
    fn set_version_duplicate_name_errors() {
        let toml = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"

[[plugin]]
name = "foo"
source = "github:other/foo"
version = "2.0.0"
"#;
        let mut doc = toml.parse::<DocumentMut>().unwrap();
        let err = set_plugin_version(&mut doc, "foo", "3.0.0").unwrap_err();
        assert!(err.contains("multiple"), "err: {}", err);
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p yosh-plugin-manager update::tests::set_version_`
Expected: 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/yosh-plugin-manager/src/update.rs
git commit -m "$(cat <<'EOF'
test(plugin-manager): defensive error coverage for set_plugin_version

Pins behavior for empty-config (no plugin key), malformed plugin key
(non-array), and duplicate-name (multi-match) inputs. The duplicate
case in particular guards against silently rewriting the wrong entry
when name uniqueness is violated.

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Implement `update()` orchestration + first orchestration test

**Files:**
- Modify: `crates/yosh-plugin-manager/src/update.rs`

- [ ] **Step 1: Replace the `update` stub with the full orchestration logic**

Replace the `update` function body in `crates/yosh-plugin-manager/src/update.rs`:

```rust
pub fn update(
    config_path: &Path,
    name_filter: Option<&str>,
    client: &GitHubClient,
) -> Result<UpdateOutcome, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("{}: {}", config_path.display(), e))?;
    let mut doc: DocumentMut = content
        .parse()
        .map_err(|e| format!("{}: {}", config_path.display(), e))?;

    let decls = config::load_config(config_path)?;

    let mut results = Vec::with_capacity(decls.len());
    let mut any_updated = false;

    for decl in &decls {
        if name_filter.is_some_and(|f| decl.name != f) {
            results.push(PluginUpdateResult {
                name: decl.name.clone(),
                status: UpdateStatus::Skipped(SkipReason::NotMatched),
            });
            continue;
        }

        let (owner, repo) = match &decl.source {
            config::PluginSource::GitHub { owner, repo } => (owner, repo),
            config::PluginSource::Local { .. } => {
                results.push(PluginUpdateResult {
                    name: decl.name.clone(),
                    status: UpdateStatus::Skipped(SkipReason::LocalSource),
                });
                continue;
            }
        };

        let current = match decl.version.as_deref() {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => {
                results.push(PluginUpdateResult {
                    name: decl.name.clone(),
                    status: UpdateStatus::Skipped(SkipReason::NoCurrentVersion),
                });
                continue;
            }
        };

        let status = match client.latest_version(owner, repo) {
            Ok(latest) if latest == current => UpdateStatus::AlreadyLatest { current },
            Ok(latest) => match set_plugin_version(&mut doc, &decl.name, &latest) {
                Ok(()) => {
                    any_updated = true;
                    UpdateStatus::Updated { from: current, to: latest }
                }
                Err(e) => UpdateStatus::Failed(e),
            },
            Err(e) => UpdateStatus::Failed(e),
        };

        results.push(PluginUpdateResult {
            name: decl.name.clone(),
            status,
        });
    }

    if any_updated {
        std::fs::write(config_path, doc.to_string())
            .map_err(|e| format!("write {}: {}", config_path.display(), e))?;
    }

    Ok(UpdateOutcome { results, any_updated })
}
```

- [ ] **Step 2: Add `into_client` to `GitHubClientWithBase`**

Edit `crates/yosh-plugin-manager/src/github.rs`. Inside the `#[cfg(test)] impl GitHubClientWithBase { ... }` block (around line 191-218), append after the existing `download` method:

```rust
    /// Unwrap to a plain `GitHubClient` so callers that need the
    /// concrete type (e.g. `update::update`'s signature) can be driven
    /// against a mockito server in tests.
    pub fn into_client(self) -> GitHubClient {
        self.inner
    }
```

- [ ] **Step 3: Add `update_skips_local_sources` test (no API call expected)**

Append inside `mod tests { ... }` in `crates/yosh-plugin-manager/src/update.rs`:

```rust
    use crate::github::GitHubClientWithBase;

    #[test]
    fn update_skips_local_sources() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        // Stage a local plugin file so config::load_config doesn't trip on the path.
        let plugin_file = dir.path().join("local.wasm");
        std::fs::write(&plugin_file, b"\0asm\x01\0\0\0").unwrap();
        std::fs::write(
            &config_path,
            format!(
                r#"[[plugin]]
name = "local-only"
source = "local:{}"
"#,
                plugin_file.display()
            ),
        )
        .unwrap();

        // Point at an unreachable base; if update tries to call out, the
        // test would either hang or fail. LocalSource skip should bypass.
        let client = GitHubClientWithBase::new("http://127.0.0.1:1").into_client();
        let outcome = update(&config_path, None, &client).unwrap();

        assert_eq!(outcome.results.len(), 1);
        assert!(matches!(
            outcome.results[0].status,
            UpdateStatus::Skipped(SkipReason::LocalSource)
        ));
        assert!(!outcome.any_updated);
    }
```

- [ ] **Step 4: Run the new test**

Run: `cargo test -p yosh-plugin-manager update::tests::update_skips_local_sources`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 5: Run all `set_version_` and `update_` tests in update.rs**

Run: `cargo test -p yosh-plugin-manager update::tests`
Expected: 9 tests pass (8 from earlier + the new orchestration smoke).

- [ ] **Step 6: Commit**

```bash
git add crates/yosh-plugin-manager/src/update.rs crates/yosh-plugin-manager/src/github.rs
git commit -m "$(cat <<'EOF'
feat(plugin-manager): implement update() orchestration

Reads plugins.toml once, loops over decls applying name_filter / local
skips / GitHub latest-version calls / per-plugin set_plugin_version
mutations, and writes the document at most once if anything changed.

Adds a test-only into_client helper on GitHubClientWithBase so update.rs
tests can pass a mockito-backed GitHubClient through update()'s
signature.

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Migrate `cmd_update` in lib.rs to printer-only

**Files:**
- Modify: `crates/yosh-plugin-manager/src/lib.rs:134-194`

- [ ] **Step 1: Replace `cmd_update` body**

In `crates/yosh-plugin-manager/src/lib.rs`, replace the entire current `fn cmd_update(...) -> i32 { ... }` (lines 134-194) with:

```rust
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
```

- [ ] **Step 2: Build the crate**

Run: `cargo build -p yosh-plugin-manager`
Expected: builds cleanly. No `unused` warnings on `update.rs` types (they are now consumed by `cmd_update`).

- [ ] **Step 3: Run the full plugin-manager test suite**

Run: `cargo test -p yosh-plugin-manager`
Expected: all tests pass — pre-existing tests in `config.rs`, `install.rs`, `github.rs`, `lockfile.rs`, etc. continue to pass alongside the new `update::tests` module. (Should observe ~50+ tests passing depending on the existing suite size.)

- [ ] **Step 4: Commit**

```bash
git add crates/yosh-plugin-manager/src/lib.rs
git commit -m "$(cat <<'EOF'
refactor(plugin-manager): cmd_update delegates to update::update

Migrates the printer-only logic out of lib.rs::cmd_update and into the
new update.rs module. Same observable behavior (per-plugin status
lines, cmd_sync(false) on any_updated, exit 2 on fatal config I/O
error), but the version rewrite is now a structural toml_edit mutation
addressed by plugin name rather than a String::replacen of the version
literal.

Fixes the silent-corruption bug when two plugins share a version
string. See spec for invariants and tests.

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Integration tests with mockito (filter, no-changes-no-touch, partial failure)

**Files:**
- Modify: `crates/yosh-plugin-manager/src/update.rs`

- [ ] **Step 1: Add tests #9, #10, #11 to the `mod tests` block**

Append inside `mod tests { ... }` in `crates/yosh-plugin-manager/src/update.rs`:

```rust
    #[test]
    fn update_name_filter_only_matches() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        std::fs::write(
            &config_path,
            r#"[[plugin]]
name = "alpha"
source = "github:owner/alpha"
version = "1.0.0"

[[plugin]]
name = "beta"
source = "github:owner/beta"
version = "1.0.0"
"#,
        )
        .unwrap();

        let mut server = mockito::Server::new();
        // Only beta should be queried.
        let _m_beta = server
            .mock("GET", "/repos/owner/beta/releases/latest")
            .with_status(200)
            .with_body(r#"{"tag_name": "v2.0.0"}"#)
            .create();

        let client = GitHubClientWithBase::new(&server.url()).into_client();
        let outcome = update(&config_path, Some("beta"), &client).unwrap();

        let alpha = outcome.results.iter().find(|r| r.name == "alpha").unwrap();
        let beta = outcome.results.iter().find(|r| r.name == "beta").unwrap();
        assert!(matches!(
            alpha.status,
            UpdateStatus::Skipped(SkipReason::NotMatched)
        ));
        assert!(matches!(beta.status, UpdateStatus::Updated { .. }));

        let after = std::fs::read_to_string(&config_path).unwrap();
        let reparsed = after.parse::<DocumentMut>().unwrap();
        let plugins = reparsed["plugin"].as_array_of_tables().unwrap();
        let alpha_tbl = plugins
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("alpha"))
            .unwrap();
        let beta_tbl = plugins
            .iter()
            .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("beta"))
            .unwrap();
        assert_eq!(
            alpha_tbl.get("version").and_then(|v| v.as_str()),
            Some("1.0.0"),
            "alpha should be untouched"
        );
        assert_eq!(
            beta_tbl.get("version").and_then(|v| v.as_str()),
            Some("2.0.0"),
            "beta should be updated"
        );
    }

    #[test]
    fn update_no_changes_does_not_touch_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        let original = r#"[[plugin]]
name = "foo"
source = "github:owner/foo"
version = "1.0.0"
"#;
        std::fs::write(&config_path, original).unwrap();

        let mut server = mockito::Server::new();
        // Latest equals current: no rewrite.
        let _m = server
            .mock("GET", "/repos/owner/foo/releases/latest")
            .with_status(200)
            .with_body(r#"{"tag_name": "v1.0.0"}"#)
            .create();

        let client = GitHubClientWithBase::new(&server.url()).into_client();
        let outcome = update(&config_path, None, &client).unwrap();

        assert!(!outcome.any_updated);
        assert!(matches!(
            outcome.results[0].status,
            UpdateStatus::AlreadyLatest { .. }
        ));

        let after = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(after, original, "file content must be byte-identical");
    }

    #[test]
    fn update_partial_failure_persists_successes() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("plugins.toml");
        std::fs::write(
            &config_path,
            r#"[[plugin]]
name = "good"
source = "github:owner/good"
version = "1.0.0"

[[plugin]]
name = "bad"
source = "github:owner/bad"
version = "1.0.0"
"#,
        )
        .unwrap();

        let mut server = mockito::Server::new();
        let _m_good = server
            .mock("GET", "/repos/owner/good/releases/latest")
            .with_status(200)
            .with_body(r#"{"tag_name": "v2.0.0"}"#)
            .create();
        let _m_bad = server
            .mock("GET", "/repos/owner/bad/releases/latest")
            .with_status(404)
            .create();

        let client = GitHubClientWithBase::new(&server.url()).into_client();
        let outcome = update(&config_path, None, &client).unwrap();

        let good = outcome.results.iter().find(|r| r.name == "good").unwrap();
        let bad = outcome.results.iter().find(|r| r.name == "bad").unwrap();
        assert!(matches!(good.status, UpdateStatus::Updated { .. }));
        assert!(
            matches!(&bad.status, UpdateStatus::Failed(_)),
            "bad should be Failed, got: {:?}",
            bad.status
        );

        let after = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            after.contains(r#"version = "2.0.0""#),
            "good's update must be persisted, got:\n{}",
            after
        );
    }
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test -p yosh-plugin-manager update::tests::update_`
Expected: 4 `update_*` tests pass (the local-source skip from Task 5 plus the three new mockito-backed tests).

- [ ] **Step 3: Run the full plugin-manager suite to confirm no regressions**

Run: `cargo test -p yosh-plugin-manager`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/yosh-plugin-manager/src/update.rs
git commit -m "$(cat <<'EOF'
test(plugin-manager): mockito integration tests for update orchestration

Covers name-filter (other plugins untouched), no-change (file
byte-identical when all already latest), and partial-failure
(per-plugin Failed status; succeeded sibling persisted).

Spec: docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md
Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Delete completed TODO entry + final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Delete the now-completed line in `TODO.md`**

Remove the line at `TODO.md:59`:

```markdown
- [ ] `yosh-plugin update`: version replacement uses naive `String::replacen` which may target wrong plugin if two share the same version — consider using `toml_edit` for TOML-preserving edits (`crates/yosh-plugin-manager/src/main.rs`)
```

(Note: the path reference in the original TODO entry says `src/main.rs` which is incorrect — the code lived in `src/lib.rs`. Either way, the entry is being deleted, not corrected.)

- [ ] **Step 2: Run the full workspace test suite to confirm no global regressions**

Run: `cargo test -p yosh-plugin-manager`
Expected: all tests pass.

(Per CLAUDE.md, plain `cargo build`/`cargo test` skips wasm test-plugin members. Running on `-p yosh-plugin-manager` exercises only the affected crate, which is sufficient for this change scope.)

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(todo): remove completed yosh-plugin update toml_edit entry

Replaced with structural toml_edit::DocumentMut rewrite in
crates/yosh-plugin-manager/src/update.rs (see spec
docs/superpowers/specs/2026-04-28-plugin-update-toml-edit-design.md).

Original task: "TODO.md の Plugin System Enhancements の中から優先度が高い
ものを選んで対応して下さい".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Verification Summary

After Task 8 completes, the branch should:

1. Have a new `crates/yosh-plugin-manager/src/update.rs` module with 11 unit tests (8 `set_version_*`, 4 `update_*`).
2. Have `cmd_update` in `lib.rs` reduced to a printer that delegates to `update::update`.
3. Preserve the existing CLI behavior: same per-plugin status lines, same `cmd_sync(false)` invocation on any update, same exit codes.
4. Eliminate the same-version sibling silent-corruption bug at the structural level (`set_plugin_version` matches by `name`, errors on duplicates).
5. Have the `TODO.md` entry deleted per CLAUDE.md convention.

Final check command:

```bash
cargo test -p yosh-plugin-manager
```

Expected: all tests pass, including the new `update::tests::*` set.
