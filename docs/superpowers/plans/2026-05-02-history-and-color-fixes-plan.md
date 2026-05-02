# History and Color Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three TODO items in interactive mode: surface `history.save` errors, support `HISTCONTROL` colon-separated values, and honor `CLICOLOR=0` to disable colors.

**Architecture:** Three independent commits on `main`. Each touches a single file (or file pair) and is independently testable. TDD throughout. No cross-task dependencies.

**Tech Stack:** Rust 2024 edition, `std::fs`, `std::io`, `std::collections::HashSet`, `tempfile` for tests, `nix` for `isatty`.

**Spec:** `docs/superpowers/specs/2026-05-02-history-and-color-fixes-design.md`

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src/interactive/history.rs` | History struct, persistence, HISTCONTROL filtering | Modify `save()` signature + tests; modify `add()` to parse colon-separated HISTCONTROL + tests |
| `src/interactive/mod.rs` | Repl driver, history-save call site | Modify line 310-315 to handle `Result` from `save()` |
| `src/main.rs` | `should_colorize()` env-detection logic | Insert `CLICOLOR=0` check |
| `TODO.md` | Project task tracker | Remove the three completed items |

No new files. No restructuring of existing files.

---

## Task 1: `history.save()` returns `Result` and caller warns on error

**Files:**
- Modify: `src/interactive/history.rs:1-3` (imports), `src/interactive/history.rs:129-145` (`save` method), `src/interactive/history.rs:253-285` (existing `save` tests)
- Modify: `src/interactive/mod.rs:310-315` (call site)
- Test: `src/interactive/history.rs` `tests` module (add new test)

- [ ] **Step 1.1: Write a failing test for the new error-returning behavior**

Add to `src/interactive/history.rs` inside `mod tests`, after the existing `test_save_histfilesize_truncation`:

```rust
#[test]
fn test_save_returns_err_on_unwritable_parent() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let readonly_parent = dir.path().join("readonly");
    std::fs::create_dir(&readonly_parent).unwrap();
    let mut perms = std::fs::metadata(&readonly_parent).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(&readonly_parent, perms).unwrap();

    let path = readonly_parent.join("history");
    let mut h = History::new();
    h.add("cmd1", 500, "");

    let result = h.save(&path, 500);

    // Restore writable mode so tempdir cleanup can succeed
    let mut restore = std::fs::metadata(&readonly_parent).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(&readonly_parent, restore).ok();

    // On macOS/Linux as non-root, File::create inside a 0o555 dir fails.
    // If running as root (e.g., some CI containers), root bypasses mode bits and the
    // create succeeds; in that case we skip the assertion since the bug isn't
    // reproducible and the codepath is still exercised.
    if nix::unistd::geteuid().is_root() {
        eprintln!("test_save_returns_err_on_unwritable_parent: skipped (running as root)");
        return;
    }
    assert!(result.is_err(), "expected Err when parent dir is read-only, got {:?}", result);
}
```

- [ ] **Step 1.2: Run test to verify it fails to compile**

Run: `cargo test --lib interactive::history::tests::test_save_returns_err_on_unwritable_parent 2>&1 | head -40`
Expected: compile error — `save` returns `()` not `Result`, so `result.is_err()` does not exist on `()`. Also, existing `test_save_and_load` and `test_save_histfilesize_truncation` still call `h.save(...)` without `.unwrap()` so they may also start failing once the signature changes; we will fix them in Step 1.3.

- [ ] **Step 1.3: Change `save` to return `io::Result<()>` and update existing tests**

In `src/interactive/history.rs`, change line 2 from:

```rust
use std::io::{BufRead, BufReader, Write};
```

to:

```rust
use std::io::{self, BufRead, BufReader, Write};
```

Replace the `save` method (lines 129-145) with:

```rust
pub fn save(&self, path: &Path, histfilesize: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    let start = if histfilesize > 0 && self.entries.len() > histfilesize {
        self.entries.len() - histfilesize
    } else {
        0
    };
    for entry in &self.entries[start..] {
        writeln!(file, "{}", entry)?;
    }
    Ok(())
}
```

Update existing `test_save_and_load` (line 260) — change `h.save(&path, 500);` to `h.save(&path, 500).unwrap();`.
Update existing `test_save_histfilesize_truncation` (line 281) — change `h.save(&path, 3);` to `h.save(&path, 3).unwrap();`.

- [ ] **Step 1.4: Run all `history` tests to verify they compile and pass**

Run: `cargo test --lib interactive::history::tests 2>&1 | tail -20`
Expected: all tests pass, including the new `test_save_returns_err_on_unwritable_parent`.

- [ ] **Step 1.5: Update the call site in `src/interactive/mod.rs` to log warning on error**

Find lines 310-315 in `src/interactive/mod.rs`:

```rust
        if !histfile.is_empty() {
            self.executor
                .env
                .history
                .save(std::path::Path::new(&histfile), histfilesize);
        }
```

Replace with:

```rust
        if !histfile.is_empty() {
            if let Err(e) = self
                .executor
                .env
                .history
                .save(std::path::Path::new(&histfile), histfilesize)
            {
                eprintln!(
                    "yosh: warning: cannot save history to {}: {}",
                    histfile, e
                );
            }
        }
```

- [ ] **Step 1.6: Build the full crate to confirm no other callers broke**

Run: `cargo build 2>&1 | tail -20`
Expected: build succeeds (no warnings about unused `Result`, no compile errors).

- [ ] **Step 1.7: Run full library test suite for interactive module**

Run: `cargo test --lib interactive:: 2>&1 | tail -10`
Expected: all interactive tests pass.

- [ ] **Step 1.8: Commit**

```bash
git add src/interactive/history.rs src/interactive/mod.rs
git commit -m "$(cat <<'EOF'
fix(interactive): surface history.save errors to stderr

Change History::save to return io::Result<()> instead of swallowing
fs::File::create and writeln! failures. The shell-exit caller logs a
warning to stderr when persistence fails, so users know when their
history was not written (disk full, permission denied, etc).

Original prompt: TODO.md priority A — user-facing bug fixes.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `HISTCONTROL` colon-separated values

**Files:**
- Modify: `src/interactive/history.rs:1-3` (imports — add `HashSet`), `src/interactive/history.rs:52-76` (`add` method)
- Test: `src/interactive/history.rs` `tests` module (add four new tests)

- [ ] **Step 2.1: Write failing tests for colon-separated and unknown-token behavior**

Add to `src/interactive/history.rs` inside `mod tests`, after `test_add_ignoreboth` (line 186):

```rust
#[test]
fn test_add_histcontrol_colon_separated_dups_and_space() {
    let mut h = History::new();
    h.add("ls", 500, "ignoredups:ignorespace");
    h.add("ls", 500, "ignoredups:ignorespace");
    h.add(" secret", 500, "ignoredups:ignorespace");
    h.add("pwd", 500, "ignoredups:ignorespace");
    assert_eq!(h.entries(), &["ls", "pwd"]);
}

#[test]
fn test_add_histcontrol_colon_separated_reverse_order() {
    let mut h = History::new();
    h.add("ls", 500, "ignorespace:ignoredups");
    h.add("ls", 500, "ignorespace:ignoredups");
    h.add(" secret", 500, "ignorespace:ignoredups");
    h.add("pwd", 500, "ignorespace:ignoredups");
    assert_eq!(h.entries(), &["ls", "pwd"]);
}

#[test]
fn test_add_histcontrol_unknown_token_ignored() {
    let mut h = History::new();
    h.add("ls", 500, "foo:ignoredups");
    h.add("ls", 500, "foo:ignoredups");
    h.add(" visible", 500, "foo:ignoredups");
    assert_eq!(h.entries(), &["ls", " visible"]);
}

#[test]
fn test_add_histcontrol_only_unknown_tokens() {
    let mut h = History::new();
    h.add("ls", 500, "foo:bar");
    h.add("ls", 500, "foo:bar");
    h.add(" leading_space", 500, "foo:bar");
    assert_eq!(h.entries(), &["ls", "ls", " leading_space"]);
}
```

- [ ] **Step 2.2: Run new tests to verify they fail**

Run: `cargo test --lib interactive::history::tests::test_add_histcontrol_colon 2>&1 | tail -30`
Expected: failures on the four colon-separated tests because the current `add` does only equality comparison; e.g., `"ignoredups:ignorespace" == "ignoredups"` is false, so dups are not filtered.

- [ ] **Step 2.3: Implement colon-separated parsing in `add`**

In `src/interactive/history.rs`, change the import line from:

```rust
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
```

to:

```rust
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
```

Replace the `add` method body (lines 52-76 after the Step 1 changes — same range pre-Step 1):

```rust
pub fn add(&mut self, line: &str, histsize: usize, histcontrol: &str) {
    if line.is_empty() {
        return;
    }

    let tokens: HashSet<&str> = histcontrol.split(':').collect();
    let ignore_space = tokens.contains("ignorespace") || tokens.contains("ignoreboth");
    let ignore_dups = tokens.contains("ignoredups") || tokens.contains("ignoreboth");

    if ignore_space && line.starts_with(' ') {
        return;
    }

    if ignore_dups && self.entries.last().map(|s| s.as_str()) == Some(line) {
        return;
    }

    self.entries.push(line.to_string());

    // Truncate to histsize (remove oldest entries)
    if histsize > 0 && self.entries.len() > histsize {
        let excess = self.entries.len() - histsize;
        self.entries.drain(..excess);
    }
}
```

- [ ] **Step 2.4: Run the four new tests to verify they pass**

Run: `cargo test --lib interactive::history::tests::test_add_histcontrol_colon 2>&1 | tail -20`
Expected: all four colon-separated tests pass.

- [ ] **Step 2.5: Run all `add`-related tests including the existing four to confirm no regression**

Run: `cargo test --lib interactive::history::tests::test_add 2>&1 | tail -20`
Expected: all `test_add_*` tests pass (including `test_add_basic`, `_ignoredups`, `_ignorespace`, `_ignoreboth`, `_histsize_truncation`, `_empty_line_skipped`, plus the four new ones).

- [ ] **Step 2.6: Run full library test suite**

Run: `cargo test --lib 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 2.7: Commit**

```bash
git add src/interactive/history.rs
git commit -m "$(cat <<'EOF'
feat(interactive): support HISTCONTROL colon-separated values

Parse HISTCONTROL as a colon-separated set of tokens (ignoredups,
ignorespace, ignoreboth). Matches bash 5+ behavior:
HISTCONTROL=ignoredups:ignorespace now filters both. Unknown tokens
are silently ignored. Single-value forms continue to work unchanged.

erasedups is intentionally not implemented (out of scope).

Original prompt: TODO.md priority A — user-facing bash compat.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `CLICOLOR=0` disables colors on TTY

**Files:**
- Modify: `src/main.rs:20-30` (`should_colorize` function)

This task has no automated test. `should_colorize` reads `std::env` directly and is private to `main.rs`. Adding tests requires either `std::env::set_var` (process-global, racy under parallel test execution) or a refactor to inject env, both out of scope per spec. Verification is manual.

- [ ] **Step 3.1: Insert `CLICOLOR=0` check between `CLICOLOR_FORCE` and `isatty`**

In `src/main.rs`, replace lines 20-30:

```rust
fn should_colorize() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Some(val) = std::env::var_os("CLICOLOR_FORCE") {
        if val != "0" {
            return true;
        }
    }
    nix::unistd::isatty(std::io::stdout()).unwrap_or(false)
}
```

with:

```rust
fn should_colorize() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Some(val) = std::env::var_os("CLICOLOR_FORCE") {
        if val != "0" {
            return true;
        }
    }
    if let Some(val) = std::env::var_os("CLICOLOR") {
        if val == "0" {
            return false;
        }
    }
    nix::unistd::isatty(std::io::stdout()).unwrap_or(false)
}
```

- [ ] **Step 3.2: Build to confirm compiles**

Run: `cargo build 2>&1 | tail -10`
Expected: build succeeds.

- [ ] **Step 3.3: Manual smoke test — `CLICOLOR=0` disables colors**

Run: `CLICOLOR=0 cargo run --quiet -- --help 2>&1 | head -5 | cat -v`
Expected: no `^[[` ANSI escape sequences in output. The header line should appear as plain text `yosh - A POSIX-compliant shell` without bold codes.

- [ ] **Step 3.4: Manual smoke test — TTY default unchanged**

Run: `cargo run --quiet -- --help 2>&1 | head -5 | cat -v` (without `CLICOLOR=0`)
Expected: ANSI escape sequences present (e.g., `^[[1m...^[[0m`) for the bold header — TTY default behavior preserved.
Note: when `cat -v` is in the pipeline, stdout is no longer a TTY, so colors are NOT expected here. To verify the TTY-on case, run `cargo run --quiet -- --help` directly in the terminal and visually confirm bold/yellow text.

- [ ] **Step 3.5: Manual smoke test — `NO_COLOR` still wins over `CLICOLOR=1`**

Run: `NO_COLOR=1 CLICOLOR=1 cargo run --quiet -- --help 2>&1 | head -5 | cat -v`
Expected: no ANSI escape sequences.

- [ ] **Step 3.6: Manual smoke test — `CLICOLOR_FORCE=1` overrides `CLICOLOR=0`**

Run: `CLICOLOR_FORCE=1 CLICOLOR=0 cargo run --quiet -- --help 2>&1 | head -5 | cat -v`
Expected: ANSI escape sequences present (force wins).

- [ ] **Step 3.7: Run the full lib test suite to confirm nothing else regressed**

Run: `cargo test --lib 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 3.8: Commit**

```bash
git add src/main.rs
git commit -m "$(cat <<'EOF'
feat(interactive): honor CLICOLOR=0 to disable colors on TTY

Add CLICOLOR=0 check between CLICOLOR_FORCE and isatty(stdout) in
should_colorize(). Matches the BSD/Apple convention used by ls, git,
grep, bat, etc. NO_COLOR retains highest precedence; CLICOLOR_FORCE=1
still overrides CLICOLOR=0 (force wins, matching bat/git).

Original prompt: TODO.md priority A — common CLI convention.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Remove the three completed items from `TODO.md`

**Files:**
- Modify: `TODO.md`

- [ ] **Step 4.1: Delete the three lines from `TODO.md`**

Per CLAUDE.md and project memory: completed items are deleted, not marked `[x]`.

In `TODO.md`, delete these lines:

- Line 20: `- [ ] HISTCONTROL colon-separated values — bash supports ignoredups:ignorespace but current implementation only accepts single values like ignoreboth (src/interactive/history.rs)`
- Line 21: `- [ ] history.save() silently ignores write errors — disk-full or permission errors are swallowed (src/interactive/history.rs)`
- Line 30: `- [ ] CLICOLOR=0 support in should_colorize() — disable colors even on TTY when CLICOLOR=0 is set; many CLI tools support this alongside NO_COLOR (src/main.rs)`

(The line numbers reference the pre-edit state. After deleting line 20, what was line 21 shifts up; delete based on content match, not line number.)

- [ ] **Step 4.2: Verify the deletions and that the surrounding sections still make sense**

Run: `grep -n "HISTCONTROL colon\|history.save() silently\|CLICOLOR=0" TODO.md`
Expected: no matches (all three lines removed).

- [ ] **Step 4.3: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(todo): remove completed history and color items

Removed: HISTCONTROL colon-separated values, history.save silent
errors, CLICOLOR=0 support — all addressed in the preceding three
commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Final verification

- [ ] **Step 5.1: Full test suite (background)**

Run in background: `cargo test 2>&1 | tee /tmp/yosh-final-test.log`
Expected: all tests pass. Check `/tmp/yosh-final-test.log` summary line.
Note: this is the per-CLAUDE.md session-end test gate.

- [ ] **Step 5.2: E2E suite (background, debug build)**

Run in background: `./e2e/run_tests.sh 2>&1 | tee /tmp/yosh-final-e2e.log`
Expected: all tests pass or only pre-existing flakes (PTY-sensitive paths) — none of the three changes touch parser/expander/exec.

- [ ] **Step 5.3: Confirm git log shows four clean commits on `main`**

Run: `git log --oneline -5`
Expected: top four commits are the three feat/fix commits + the TODO chore commit, all on `main`, ahead of `origin/main`.

---

## Spec coverage check

| Spec section | Plan task |
|---|---|
| §1 history.save error propagation | Task 1 (Steps 1.1–1.8) |
| §2 HISTCONTROL colon-separated | Task 2 (Steps 2.1–2.7) |
| §3 CLICOLOR=0 support | Task 3 (Steps 3.1–3.8) |
| Implementation order: 3 commits on main | Tasks 1, 2, 3 produce one commit each; Task 4 cleans up TODO; total 4 commits |
| Risks: read-only-dir test under root | Task 1 Step 1.1 includes the `geteuid().is_root()` skip |
| Risks: HashSet allocation per add | Task 2 Step 2.3 uses `HashSet<&str>` per spec acceptance |
| Risks: CLICOLOR manual-only test | Task 3 has no auto test by design (Steps 3.3–3.6 are manual) |
