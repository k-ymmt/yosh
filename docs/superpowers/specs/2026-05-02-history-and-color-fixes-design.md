# History save error handling, HISTCONTROL colon-separated values, and CLICOLOR=0 support

**Date:** 2026-05-02
**Type:** User-facing bug fix + bash/BSD extension parity
**POSIX scope:** All three changes are POSIX-compatible (do not alter POSIX-defined behavior). HISTCONTROL and CLICOLOR are non-POSIX extensions; the history-save error path is a bug fix to an existing non-POSIX history persistence layer.

## Background

Three small interactive-mode improvements are tracked in `TODO.md`:

1. **`history.save()` silent error swallowing** (`src/interactive/history.rs:129-145`) — disk-full or permission errors when writing `HISTFILE` are silently dropped, so users lose history without warning.
2. **`HISTCONTROL` colon-separated values** (`src/interactive/history.rs:52-67`) — the existing implementation only accepts single tokens (`ignoredups`, `ignorespace`, `ignoreboth`). Bash supports `ignoredups:ignorespace`-style colon lists.
3. **`CLICOLOR=0` support in `should_colorize()`** (`src/main.rs:20-30`) — the BSD/Apple convention to disable colors via `CLICOLOR=0` is not honored. Only `NO_COLOR` and `CLICOLOR_FORCE` are checked.

All three are scoped to interactive features and do not affect POSIX shell semantics.

## Goals

- Surface history persistence failures to the user instead of swallowing them.
- Accept `HISTCONTROL` colon-separated combinations matching bash 5+ behavior for the existing `ignoredups`/`ignorespace`/`ignoreboth` tokens.
- Honor `CLICOLOR=0` to disable colors even when stdout is a TTY.

## Non-goals

- `HISTCONTROL=erasedups` (bash extension to retroactively erase prior duplicates) — out of scope; remains a TODO.
- Restructuring `should_colorize()` for testability beyond what the new branch needs.
- Surfacing history-save errors as a non-zero shell exit code.

## Design

### 1. `history.save()` — return `io::Result<()>` and warn at caller

**Change** (`src/interactive/history.rs`):

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

**Caller** (`src/interactive/mod.rs:310-315`):

```rust
if !histfile.is_empty() {
    if let Err(e) = self
        .executor
        .env
        .history
        .save(std::path::Path::new(&histfile), histfilesize)
    {
        eprintln!("yosh: warning: cannot save history to {}: {}", histfile, e);
    }
}
```

**Rationale:**
- `?` propagation makes both `create_dir_all` and individual `writeln!` errors visible. Per-entry partial-write is rare but still surfaced.
- Caller at shell-exit emits a `yosh: warning: ...` message to stderr per CLAUDE.md convention. The exit code remains untouched (this is a warning, not a failure of the user's last command).

**Tests:**
- Existing `test_save_and_load` and `test_save_histfilesize_truncation` add `.unwrap()` calls.
- New `test_save_returns_err_on_unwritable_dir` — create a temp dir, set it read-only, attempt save into it, assert `Err`.
  - Skip on Windows-style permissions (we are macOS/Linux only); use `fs::set_permissions` with mode `0o555` on the parent.

### 2. `HISTCONTROL` colon-separated values

**Change** (`src/interactive/history.rs:52-67`):

Replace the two equality checks with a token-set check:

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

    if histsize > 0 && self.entries.len() > histsize {
        let excess = self.entries.len() - histsize;
        self.entries.drain(..excess);
    }
}
```

**Rationale:**
- `split(':')` produces a single token when no colon is present, so existing `"ignoredups"`/`"ignorespace"`/`"ignoreboth"` inputs continue to work unchanged.
- Unknown tokens are ignored silently (matches bash behavior for unrecognized values).
- `ignoreboth` remains as the documented shorthand.

**Tests:**
- Existing tests (`test_add_ignoredups`, `_ignorespace`, `_ignoreboth`) continue to pass without modification.
- New `test_add_histcontrol_colon_separated_dups_and_space` — `HISTCONTROL=ignoredups:ignorespace` filters both.
- New `test_add_histcontrol_colon_separated_reverse_order` — `HISTCONTROL=ignorespace:ignoredups` filters both (order-independent).
- New `test_add_histcontrol_unknown_token_ignored` — `HISTCONTROL=foo:ignoredups` filters dups, ignores `foo`.
- New `test_add_histcontrol_only_unknown_tokens` — `HISTCONTROL=foo:bar` is a no-op (history accepts everything).

### 3. `CLICOLOR=0` in `should_colorize()`

**Change** (`src/main.rs:20-30`):

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

**Precedence (top wins):**
1. `NO_COLOR` set (any value, even empty) → disable
2. `CLICOLOR_FORCE` non-zero → enable
3. `CLICOLOR=0` → disable
4. otherwise → `isatty(stdout)`

**Rationale:**
- Matches the BSD/Apple convention used by `ls`, `git`, `grep`, `bat`, etc.
- `NO_COLOR` retains highest precedence (de-facto cross-tool standard).
- `CLICOLOR_FORCE` continues to override TTY detection upward; `CLICOLOR=0` overrides downward. They do not contradict because `CLICOLOR_FORCE` is checked first — a user who sets both `CLICOLOR_FORCE=1` and `CLICOLOR=0` gets colors (force wins), matching bat/git behavior.

**Tests:**
- `should_colorize` reads `std::env` directly and is private. Existing code has no unit test. Adding a test would require either (a) `std::env::set_var` (process-global, racy under parallel tests) or (b) a refactor to inject env. Both are out of scope for this small additive branch.
- Verification path: manual smoke test — run `CLICOLOR=0 yosh --help` and confirm no ANSI escape codes; run `yosh --help` (TTY) and confirm colors present.

## Implementation order

Three independent commits on `main` (per project convention of working directly on main):

1. `fix(interactive): surface history.save errors to stderr`
2. `feat(interactive): support HISTCONTROL colon-separated values`
3. `feat(interactive): honor CLICOLOR=0 to disable colors on TTY`

Each commit includes its own tests and passes `cargo test` independently.

## Risks

- **CLICOLOR test gap**: Manual verification only. Acceptable because the change is 4 lines and follows a well-known convention.
- **history.save read-only-dir test on CI**: `fs::set_permissions(0o555)` may behave differently in CI containers running as root (root bypasses mode bits). Use `tempfile::tempdir()` and check at runtime whether `File::create` actually fails; if it doesn't (running as root), skip the test with `eprintln!` and pass.
- **HashSet allocation per `add` call**: each history insertion now allocates a `HashSet<&str>`. Acceptable — `add` is called once per Enter keystroke, not in a hot loop.

## Open questions

None — design choices match user-confirmed POSIX-compatibility-only scope and TODO.md's stated requests.
