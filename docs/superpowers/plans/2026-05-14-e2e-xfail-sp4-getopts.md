# SP4 — `getopts` Builtin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement POSIX XCU `getopts` as a native yosh regular builtin so the 9 SP4 XFAIL tests transition to PASS, while preserving function-scope `OPTIND` save/restore semantics.

**Architecture:** New file `src/builtin/getopts.rs` exposes `builtin_getopts(args, env)` and a pure `step_getopts(...)` helper. State splits two ways: `OPTIND`/`OPTARG` live in `env.vars` (user-visible/writable); a stacked-option subcursor and the per-function saved OPTIND live as new fields on `Scope` in `src/env/vars.rs`. `ShellEnv::new` seeds `OPTIND="1"` at startup.

**Tech Stack:** Rust 2024, libc (transitively via existing builtin module), `cargo test`, `./e2e/run_tests.sh`.

**Spec:** `docs/superpowers/specs/2026-05-14-e2e-xfail-sp4-getopts-design.md`.

---

## File Structure

| Path | Status | Responsibility |
|---|---|---|
| `src/builtin/getopts.rs` | **new** | `builtin_getopts` entry + `parse_args` + pure `step_getopts` + unit tests |
| `src/builtin/mod.rs` | modify | Register `getopts` (module decl, `BUILTIN_NAMES`, `classify_builtin`, `exec_regular_builtin`) |
| `src/env/vars.rs` | modify | `Scope` gains `getopts_subindex` + `saved_optind`; `push_scope` snapshots OPTIND and resets to `"1"`; `pop_scope` restores |
| `src/env/mod.rs` | modify | `ShellEnv::new` writes `OPTIND="1"` into global vars |
| `e2e/posix_spec/4_required_builtin/getopts_basic.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/getopts_with_arg.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/getopts_stacked.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/getopts_unknown.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/getopts_missing_arg.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/getopts_optind.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/8_env_vars/OPTIND_initial_one.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/8_env_vars/OPTIND_advances.sh` | modify | Strip `# XFAIL:` line |
| `e2e/posix_spec/8_env_vars/OPTARG_set_by_getopts.sh` | modify | Strip `# XFAIL:` line |
| `TODO.md` | modify | Delete SP4 roadmap bullet + `getopts` POSIX-required-builtin bullet |

---

## Group G1 — env/vars scope plumbing

### Task 1: Extend `Scope` with `getopts_subindex` and `saved_optind`

**Files:**
- Modify: `src/env/vars.rs:29-34` (`Scope` struct) and `src/env/vars.rs:51-77` (`VarStore::new` + `from_environ` literal sites)
- Test: `src/env/vars.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block in `src/env/vars.rs`:

```rust
#[test]
fn push_scope_snapshots_optind_and_resets_to_one() {
    let mut store = VarStore::new();
    store.set("OPTIND", "5").unwrap();

    store.push_scope(vec!["a".into(), "b".into()]);
    assert_eq!(store.get("OPTIND"), Some("1"));

    store.pop_scope();
    assert_eq!(store.get("OPTIND"), Some("5"));
}

#[test]
fn push_scope_initial_subindex_is_zero() {
    let mut store = VarStore::new();
    store.push_scope(vec![]);
    assert_eq!(store.getopts_subindex(), 0);
}

#[test]
fn set_getopts_subindex_round_trips() {
    let mut store = VarStore::new();
    store.set_getopts_subindex(3);
    assert_eq!(store.getopts_subindex(), 3);
}

#[test]
fn push_scope_resets_subindex_and_pop_restores() {
    let mut store = VarStore::new();
    store.set_getopts_subindex(7);

    store.push_scope(vec![]);
    assert_eq!(store.getopts_subindex(), 0);
    store.set_getopts_subindex(2);

    store.pop_scope();
    assert_eq!(store.getopts_subindex(), 7);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p yosh --lib env::vars::tests::push_scope_snapshots_optind_and_resets_to_one`
Expected: FAIL — compile error: methods `getopts_subindex` / `set_getopts_subindex` undefined, or `OPTIND` not seeded.

- [ ] **Step 3: Extend `Scope` struct**

Replace the `Scope` struct at `src/env/vars.rs:29-34`:

```rust
/// A single scope in the scope chain.
#[derive(Debug, Clone)]
struct Scope {
    vars: HashMap<String, Variable>,
    positional_params: Vec<String>,
    /// POSIX `getopts` cursor within a stacked argv element (e.g. `-abc`).
    /// `0` means "advance to the next argv element on the next call."
    getopts_subindex: usize,
    /// `OPTIND` value snapshot saved on `push_scope`, restored on `pop_scope`.
    /// `None` outside any function call (global scope).
    saved_optind: Option<String>,
}
```

- [ ] **Step 4: Update existing `Scope { ... }` literal sites**

`src/env/vars.rs:54-58` (inside `VarStore::new`):

```rust
        VarStore {
            scopes: vec![Scope {
                vars: HashMap::new(),
                positional_params: Vec::new(),
                getopts_subindex: 0,
                saved_optind: None,
            }],
            environ_cache: None,
        }
```

`src/env/vars.rs:69-74` (inside `VarStore::from_environ`):

```rust
        VarStore {
            scopes: vec![Scope {
                vars,
                positional_params: Vec::new(),
                getopts_subindex: 0,
                saved_optind: None,
            }],
            environ_cache: None,
        }
```

- [ ] **Step 5: Rewrite `push_scope` to snapshot OPTIND and reset to "1"**

Replace `src/env/vars.rs:82-88` (`push_scope`):

```rust
    /// Push a new scope with the given positional parameters.
    /// Used for function calls.
    ///
    /// Saves the caller's current `OPTIND` value into the new scope's
    /// `saved_optind` and resets the visible `OPTIND` to `"1"`. The
    /// stacked-options subcursor starts at `0`.
    pub fn push_scope(&mut self, positional_params: Vec<String>) {
        self.environ_cache = None;
        // Snapshot caller's OPTIND (may be unset → None).
        let saved_optind = self.get("OPTIND").map(|s| s.to_string());
        self.scopes.push(Scope {
            vars: HashMap::new(),
            positional_params,
            getopts_subindex: 0,
            saved_optind,
        });
        // Set OPTIND="1" in the new (top) scope so the function body
        // sees a fresh parse position. Direct write into top scope to
        // avoid POSIX "assign in caller" semantics of `set()`.
        self.scopes
            .last_mut()
            .unwrap()
            .vars
            .insert("OPTIND".to_string(), Variable::new("1"));
    }
```

- [ ] **Step 6: Rewrite `pop_scope` to restore OPTIND**

Replace `src/env/vars.rs:92-96` (`pop_scope`):

```rust
    /// Pop the current scope, restoring the previous scope's positional
    /// parameters. Panics if only the global scope remains.
    ///
    /// Restores the caller's `OPTIND` from the popped scope's
    /// `saved_optind` snapshot (writing into whichever underlying scope
    /// already holds OPTIND, or creating it in the new top scope).
    pub fn pop_scope(&mut self) {
        self.environ_cache = None;
        assert!(self.scopes.len() > 1, "cannot pop the global scope");
        let popped = self.scopes.pop().unwrap();
        if let Some(prev_optind) = popped.saved_optind {
            // Write back into the now-current scope chain. Use `set`
            // so the value lands where OPTIND was originally defined
            // (typically scope[0]). Readonly OPTIND is not supported
            // and the assignment cannot fail in practice.
            let _ = self.set("OPTIND", prev_optind);
        }
    }
```

- [ ] **Step 7: Add `getopts_subindex` accessors**

Insert after `pop_scope` (around `src/env/vars.rs:96`):

```rust
    // ── getopts subcursor (top scope) ───────────────────────────────────

    /// Get the current scope's `getopts` stacked-options subcursor.
    pub fn getopts_subindex(&self) -> usize {
        self.scopes.last().unwrap().getopts_subindex
    }

    /// Set the current scope's `getopts` stacked-options subcursor.
    pub fn set_getopts_subindex(&mut self, value: usize) {
        self.scopes.last_mut().unwrap().getopts_subindex = value;
    }
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p yosh --lib env::vars::tests`
Expected: PASS — all `push_scope_*`/`set_getopts_subindex_*` cases plus existing scope tests.

- [ ] **Step 9: Run full lib tests**

Run: `cargo test -p yosh --lib`
Expected: PASS — no regressions in `env`, `builtin`, `exec`, etc.

- [ ] **Step 10: Commit**

```bash
git add src/env/vars.rs
git commit -m "$(cat <<'EOF'
feat(env/vars): scope-local getopts subcursor + OPTIND save/restore

POSIX `getopts` requires per-function OPTIND state. Add
`getopts_subindex` and `saved_optind` fields to `Scope`; on
`push_scope` snapshot the caller's OPTIND and seed the new scope
with OPTIND="1"; on `pop_scope` restore the snapshot. Subcursor
defaults to 0 and is independent per scope.

Prep for SP4 native `getopts` builtin.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `ShellEnv::new` seeds `OPTIND="1"` at startup

**Files:**
- Modify: `src/env/mod.rs:58-86` (`ShellEnv::new`)
- Test: `src/env/mod.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block in `src/env/mod.rs`:

```rust
#[test]
fn shell_env_new_seeds_optind_to_one() {
    let env = ShellEnv::new("yosh", vec![]);
    assert_eq!(env.vars.get("OPTIND"), Some("1"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p yosh --lib env::tests::shell_env_new_seeds_optind_to_one`
Expected: FAIL — `OPTIND` is currently unset (returns `None`).

- [ ] **Step 3: Seed `OPTIND="1"` in `ShellEnv::new`**

In `src/env/mod.rs:58-61`, after `vars.set_positional_params(args);` add:

```rust
    pub fn new(shell_name: impl Into<String>, args: Vec<String>) -> Self {
        let mut vars = VarStore::from_environ();
        vars.set_positional_params(args);
        // POSIX: "OPTIND shall be initialized to 1 when the shell is invoked."
        let _ = vars.set("OPTIND", "1");
        ShellEnv {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p yosh --lib env::tests::shell_env_new_seeds_optind_to_one`
Expected: PASS.

- [ ] **Step 5: Run full lib tests**

Run: `cargo test -p yosh --lib`
Expected: PASS — no regressions.

- [ ] **Step 6: Commit**

```bash
git add src/env/mod.rs
git commit -m "$(cat <<'EOF'
feat(env): seed OPTIND="1" at ShellEnv construction

POSIX XCU `getopts`: "OPTIND shall be initialized to 1 when the
shell is invoked." Set via the global `VarStore` so user code that
reads `$OPTIND` before any `getopts` call observes the spec value.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Group G2 — getopts builtin core

### Task 3: Create `src/builtin/getopts.rs` skeleton + `parse_args`

**Files:**
- Create: `src/builtin/getopts.rs`
- Test: `src/builtin/getopts.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test (parse_args contract)**

Create `src/builtin/getopts.rs` containing only:

```rust
//! POSIX `getopts` builtin.
//!
//! `getopts optstring var [arg ...]` — parse one option from the
//! positional parameters (or explicit `arg`s) on each call, advancing
//! `OPTIND` and setting `OPTARG`. Stacked options (`-abc`) are returned
//! one per call. A `:` prefix on `optstring` enables silent error mode.

use crate::env::ShellEnv;
use crate::error::ShellError;
use crate::parser::word::is_valid_name;

#[derive(Debug, PartialEq)]
enum ArgError {
    MissingOperands,
    InvalidVarName(String),
}

#[derive(Debug, PartialEq)]
struct ParsedArgs<'a> {
    optstring: &'a str,
    var_name: &'a str,
    operands: Vec<&'a str>,
}

fn parse_args<'a>(args: &'a [String]) -> Result<ParsedArgs<'a>, ArgError> {
    if args.len() < 2 {
        return Err(ArgError::MissingOperands);
    }
    let optstring = args[0].as_str();
    let var_name = args[1].as_str();
    if !is_valid_name(var_name) {
        return Err(ArgError::InvalidVarName(var_name.to_string()));
    }
    let operands: Vec<&str> = args[2..].iter().map(String::as_str).collect();
    Ok(ParsedArgs { optstring, var_name, operands })
}

pub fn builtin_getopts(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    // Filled in by Task 6.
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_args_minimum_two_operands() {
        assert_eq!(parse_args(&s(&[])), Err(ArgError::MissingOperands));
        assert_eq!(parse_args(&s(&["a"])), Err(ArgError::MissingOperands));
    }

    #[test]
    fn parse_args_invalid_var_name_rejected() {
        assert_eq!(
            parse_args(&s(&["a", "1foo"])),
            Err(ArgError::InvalidVarName("1foo".into()))
        );
    }

    #[test]
    fn parse_args_no_operands_means_empty_vec() {
        let args = s(&["a:", "opt"]);
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.optstring, "a:");
        assert_eq!(parsed.var_name, "opt");
        assert!(parsed.operands.is_empty());
    }

    #[test]
    fn parse_args_explicit_operands_captured() {
        let args = s(&["a:", "opt", "-a", "value"]);
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.operands, vec!["-a", "value"]);
    }
}
```

- [ ] **Step 2: Wire the module so the file compiles**

Edit `src/builtin/mod.rs:1-9` — add `pub mod getopts;`:

```rust
pub mod command;
pub mod getopts;
pub mod hash;
pub mod read;
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
pub mod r#type;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p yosh --lib builtin::getopts::tests`
Expected: PASS — 4 tests in module.

- [ ] **Step 4: Commit**

```bash
git add src/builtin/getopts.rs src/builtin/mod.rs
git commit -m "$(cat <<'EOF'
feat(builtin/getopts): module skeleton + parse_args

New `src/builtin/getopts.rs` with `parse_args` validating the
optstring/var-name pair and capturing explicit operands. `is_valid_name`
gates the var name; missing operands and bad identifiers each get a
dedicated `ArgError` variant. `builtin_getopts` stubbed to `Ok(0)`
pending the step_getopts + entry implementation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: `step_getopts` — basic options + end-of-options

**Files:**
- Modify: `src/builtin/getopts.rs`

Implements rows 1, 11, 12, 13 from spec §6.1 (single option, no-more, non-option arg, `-` operand). Sets up `GetoptsStep`, `END_OF_OPTIONS` helper, and the non-takes_arg success path.

- [ ] **Step 1: Write the failing tests**

Append inside the `mod tests` block in `src/builtin/getopts.rs`:

```rust
    #[test]
    fn step_single_option() {
        let step = step_getopts("a", &["-a"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optarg, None);
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_none());
    }

    #[test]
    fn step_end_of_options_when_index_past_operands() {
        let step = step_getopts("a", &["-a"], 2, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 1);
    }

    #[test]
    fn step_end_of_options_on_non_dash_operand() {
        let step = step_getopts("a", &["arg"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 1);
        assert_eq!(step.exit, 1);
    }

    #[test]
    fn step_end_of_options_on_lone_dash() {
        let step = step_getopts("a", &["-"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 1);
        assert_eq!(step.exit, 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p yosh --lib builtin::getopts::tests::step_`
Expected: FAIL — `step_getopts` and `GetoptsStep` undefined.

- [ ] **Step 3: Add `GetoptsStep` type and partial `step_getopts`**

Insert before the `#[cfg(test)]` block in `src/builtin/getopts.rs`:

```rust
#[derive(Debug, PartialEq)]
struct GetoptsStep {
    var_value: String,
    optarg: Option<String>,
    optind: usize,
    subindex: usize,
    exit: i32,
    stderr: Option<String>,
}

fn end_of_options(optind: usize) -> GetoptsStep {
    GetoptsStep {
        var_value: "?".to_string(),
        optarg: None,
        optind,
        subindex: 0,
        exit: 1,
        stderr: None,
    }
}

fn step_getopts(
    spec: &str,
    operands: &[&str],
    optind_in: usize,
    subindex_in: usize,
    silent: bool,
) -> GetoptsStep {
    // Drop unused-warning until Task 5 fills the body.
    let _ = silent;

    if optind_in == 0 || optind_in > operands.len() {
        return end_of_options(optind_in.max(1));
    }

    let elt = operands[optind_in - 1];

    let cursor = if subindex_in == 0 {
        if elt == "--" {
            return GetoptsStep {
                var_value: "?".to_string(),
                optarg: None,
                optind: optind_in + 1,
                subindex: 0,
                exit: 1,
                stderr: None,
            };
        }
        if !elt.starts_with('-') || elt == "-" {
            return end_of_options(optind_in);
        }
        1
    } else {
        subindex_in
    };

    let bytes = elt.as_bytes();
    let ch = bytes[cursor] as char;
    let next_cursor = cursor + 1;
    let rest_of_elt = next_cursor < bytes.len();

    let pos = spec.bytes().position(|b| b == ch as u8);
    let takes_arg = matches!(pos.and_then(|p| spec.as_bytes().get(p + 1)), Some(b':'));

    if pos.is_some() && !takes_arg {
        return GetoptsStep {
            var_value: ch.to_string(),
            optarg: None,
            optind: if rest_of_elt { optind_in } else { optind_in + 1 },
            subindex: if rest_of_elt { next_cursor } else { 0 },
            exit: 0,
            stderr: None,
        };
    }

    // Other branches (unknown, takes_arg) handled in Task 5.
    // Returning end_of_options as a placeholder is wrong but tests for
    // those branches do not exist yet.
    end_of_options(optind_in)
}
```

- [ ] **Step 4: Run tests to verify the four pass**

Run: `cargo test -p yosh --lib builtin::getopts::tests::step_`
Expected: PASS — 4 step_ tests.

- [ ] **Step 5: Run all getopts module tests**

Run: `cargo test -p yosh --lib builtin::getopts`
Expected: PASS — 8 tests (4 parse_args + 4 step_).

- [ ] **Step 6: Commit**

```bash
git add src/builtin/getopts.rs
git commit -m "$(cat <<'EOF'
feat(builtin/getopts): step_getopts basic dispatch + end-of-options

Pure `step_getopts(spec, operands, optind, subindex, silent)` covering
the trivial paths: single non-arg option success, end-of-options when
OPTIND is past the last operand, a non-dash operand, and the lone `-`.
`--` consumes-and-stops with OPTIND advanced. Remaining branches
(unknown, takes_arg, stacked) land in Task 5.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: `step_getopts` — full coverage (values, stacked, missing, unknown)

**Files:**
- Modify: `src/builtin/getopts.rs`

Implements spec §6.1 rows 2-10.

- [ ] **Step 1: Write the failing tests**

Append inside the `mod tests` block in `src/builtin/getopts.rs`:

```rust
    #[test]
    fn step_option_with_arg_same_element() {
        let step = step_getopts("a:", &["-aval"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optarg, Some("val".into()));
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_option_with_arg_next_element() {
        let step = step_getopts("a:", &["-a", "val"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optarg, Some("val".into()));
        assert_eq!(step.optind, 3);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_stacked_first() {
        let step = step_getopts("ab", &["-ab"], 1, 0, false);
        assert_eq!(step.var_value, "a");
        assert_eq!(step.optind, 1);
        assert_eq!(step.subindex, 2);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_stacked_second() {
        let step = step_getopts("ab", &["-ab"], 1, 2, false);
        assert_eq!(step.var_value, "b");
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
    }

    #[test]
    fn step_unknown_option_normal_mode() {
        let step = step_getopts("a", &["-x"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optarg, None);
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_some());
        let msg = step.stderr.unwrap();
        assert!(msg.contains("-x"), "stderr msg = {msg}");
        assert!(msg.contains("illegal option"), "stderr msg = {msg}");
    }

    #[test]
    fn step_unknown_option_silent_mode() {
        let step = step_getopts("a", &["-x"], 1, 0, true);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optarg, Some("x".into()));
        assert_eq!(step.optind, 2);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_none());
    }

    #[test]
    fn step_missing_arg_normal_mode() {
        let step = step_getopts("a:", &["-a"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optarg, None);
        assert_eq!(step.optind, 2);
        assert_eq!(step.subindex, 0);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_some());
        let msg = step.stderr.unwrap();
        assert!(msg.contains("requires an argument"), "stderr msg = {msg}");
        assert!(msg.contains("a"), "stderr msg = {msg}");
    }

    #[test]
    fn step_missing_arg_silent_mode() {
        let step = step_getopts("a:", &["-a"], 1, 0, true);
        assert_eq!(step.var_value, ":");
        assert_eq!(step.optarg, Some("a".into()));
        assert_eq!(step.optind, 2);
        assert_eq!(step.exit, 0);
        assert!(step.stderr.is_none());
    }

    #[test]
    fn step_double_dash_advances_optind() {
        let step = step_getopts("a", &["--"], 1, 0, false);
        assert_eq!(step.var_value, "?");
        assert_eq!(step.optind, 2);
        assert_eq!(step.exit, 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p yosh --lib builtin::getopts::tests::step_`
Expected: FAIL — 6 of the 9 new tests fail (unknown/missing/with-arg/stacked-second paths still return placeholder `end_of_options`).

- [ ] **Step 3: Replace `step_getopts` body with full implementation**

Replace the entire `step_getopts` function in `src/builtin/getopts.rs` with:

```rust
fn step_getopts(
    spec: &str,
    operands: &[&str],
    optind_in: usize,
    subindex_in: usize,
    silent: bool,
) -> GetoptsStep {
    if optind_in == 0 || optind_in > operands.len() {
        return end_of_options(optind_in.max(1));
    }

    let elt = operands[optind_in - 1];

    let cursor = if subindex_in == 0 {
        if elt == "--" {
            return GetoptsStep {
                var_value: "?".to_string(),
                optarg: None,
                optind: optind_in + 1,
                subindex: 0,
                exit: 1,
                stderr: None,
            };
        }
        if !elt.starts_with('-') || elt == "-" {
            return end_of_options(optind_in);
        }
        1
    } else {
        subindex_in
    };

    let bytes = elt.as_bytes();
    let ch = bytes[cursor] as char;
    let next_cursor = cursor + 1;
    let rest_of_elt = next_cursor < bytes.len();

    let pos = spec.bytes().position(|b| b == ch as u8);

    // Unknown option
    if pos.is_none() {
        let next_optind = if rest_of_elt { optind_in } else { optind_in + 1 };
        let next_sub = if rest_of_elt { next_cursor } else { 0 };
        if silent {
            return GetoptsStep {
                var_value: "?".to_string(),
                optarg: Some(ch.to_string()),
                optind: next_optind,
                subindex: next_sub,
                exit: 0,
                stderr: None,
            };
        }
        return GetoptsStep {
            var_value: "?".to_string(),
            optarg: None,
            optind: next_optind,
            subindex: next_sub,
            exit: 0,
            stderr: Some(format!("-{}: illegal option", ch)),
        };
    }

    let pos = pos.unwrap();
    let takes_arg = matches!(spec.as_bytes().get(pos + 1), Some(b':'));

    // Known, no-arg option
    if !takes_arg {
        return GetoptsStep {
            var_value: ch.to_string(),
            optarg: None,
            optind: if rest_of_elt { optind_in } else { optind_in + 1 },
            subindex: if rest_of_elt { next_cursor } else { 0 },
            exit: 0,
            stderr: None,
        };
    }

    // Known, takes argument — argument inside same element
    if rest_of_elt {
        let arg = &elt[next_cursor..];
        return GetoptsStep {
            var_value: ch.to_string(),
            optarg: Some(arg.to_string()),
            optind: optind_in + 1,
            subindex: 0,
            exit: 0,
            stderr: None,
        };
    }

    // Argument in next element
    if optind_in + 1 > operands.len() {
        // Missing
        if silent {
            return GetoptsStep {
                var_value: ":".to_string(),
                optarg: Some(ch.to_string()),
                optind: optind_in + 1,
                subindex: 0,
                exit: 0,
                stderr: None,
            };
        }
        return GetoptsStep {
            var_value: "?".to_string(),
            optarg: None,
            optind: optind_in + 1,
            subindex: 0,
            exit: 0,
            stderr: Some(format!("option requires an argument -- {}", ch)),
        };
    }

    let arg = operands[optind_in];
    GetoptsStep {
        var_value: ch.to_string(),
        optarg: Some(arg.to_string()),
        optind: optind_in + 2,
        subindex: 0,
        exit: 0,
        stderr: None,
    }
}
```

- [ ] **Step 4: Run all getopts tests to verify they pass**

Run: `cargo test -p yosh --lib builtin::getopts`
Expected: PASS — 17 tests (4 parse_args + 13 step_).

- [ ] **Step 5: Run full lib tests**

Run: `cargo test -p yosh --lib`
Expected: PASS — no regressions in unrelated modules.

- [ ] **Step 6: Commit**

```bash
git add src/builtin/getopts.rs
git commit -m "$(cat <<'EOF'
feat(builtin/getopts): step_getopts full POSIX coverage

Adds the remaining branches of the pure step function: option-with-arg
in same element (`-aval`) and next element (`-a val`), stacked
follow-up (subindex > 0), unknown option in both normal and silent
modes, missing argument in both modes. Matches spec §4-§6 row table.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: `builtin_getopts` entry + dispatch wiring

**Files:**
- Modify: `src/builtin/getopts.rs` (replace stub)
- Modify: `src/builtin/mod.rs:13-19` (BUILTIN_NAMES), `src/builtin/mod.rs:34-42` (`classify_builtin`), `src/builtin/mod.rs:45-86` (`exec_regular_builtin`)

- [ ] **Step 1: Write the failing integration tests**

Append inside the `mod tests` block in `src/builtin/getopts.rs`:

```rust
    use crate::env::ShellEnv;

    fn make_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn builtin_dispatches_simple_option_from_positional() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-a".into()]);
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
        assert_eq!(env.vars.get("OPTIND"), Some("2"));
    }

    #[test]
    fn builtin_sets_optarg_for_takes_arg() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-a".into(), "value".into()]);
        let rc = super::builtin_getopts(&s(&["a:", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
        assert_eq!(env.vars.get("OPTARG"), Some("value"));
        assert_eq!(env.vars.get("OPTIND"), Some("3"));
    }

    #[test]
    fn builtin_explicit_operands_override_positional() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-x".into()]);
        let rc = super::builtin_getopts(
            &s(&["a", "opt", "-a"]),
            &mut env,
        ).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
    }

    #[test]
    fn builtin_stacked_two_calls() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-ab".into()]);
        let args = s(&["ab", "opt"]);

        let rc1 = super::builtin_getopts(&args, &mut env).unwrap();
        assert_eq!(rc1, 0);
        assert_eq!(env.vars.get("opt"), Some("a"));
        assert_eq!(env.vars.get("OPTIND"), Some("1"));

        let rc2 = super::builtin_getopts(&args, &mut env).unwrap();
        assert_eq!(rc2, 0);
        assert_eq!(env.vars.get("opt"), Some("b"));
        assert_eq!(env.vars.get("OPTIND"), Some("2"));
    }

    #[test]
    fn builtin_end_of_options_returns_one() {
        let mut env = make_env();
        env.vars.set_positional_params(vec!["arg".into()]);
        let rc = super::builtin_getopts(&s(&["a", "opt"]), &mut env).unwrap();
        assert_eq!(rc, 1);
        assert_eq!(env.vars.get("opt"), Some("?"));
    }

    #[test]
    fn builtin_user_resets_optind_to_one() {
        // Mid-stacked-parse: subindex is 2, OPTIND is 1.
        // User writes OPTIND=1 explicitly → next call must restart.
        let mut env = make_env();
        env.vars.set_positional_params(vec!["-ab".into()]);
        let args = s(&["ab", "opt"]);
        super::builtin_getopts(&args, &mut env).unwrap(); // consumes 'a', sub=2
        assert_eq!(env.vars.getopts_subindex(), 2);

        env.vars.set("OPTIND", "1").unwrap();
        super::builtin_getopts(&args, &mut env).unwrap();
        // After reset, parse restarts and 'a' is returned again.
        assert_eq!(env.vars.get("opt"), Some("a"));
    }

    #[test]
    fn builtin_missing_operands_returns_two() {
        let mut env = make_env();
        let rc = super::builtin_getopts(&s(&[]), &mut env).unwrap();
        assert_eq!(rc, 2);
    }

    #[test]
    fn builtin_invalid_var_name_returns_two() {
        let mut env = make_env();
        let rc = super::builtin_getopts(&s(&["a", "1foo"]), &mut env).unwrap();
        assert_eq!(rc, 2);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p yosh --lib builtin::getopts::tests::builtin_`
Expected: FAIL — `builtin_getopts` is still the `Ok(0)` stub.

- [ ] **Step 3: Replace `builtin_getopts` with real implementation**

Replace the stub in `src/builtin/getopts.rs` with:

```rust
pub fn builtin_getopts(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(ArgError::MissingOperands) => {
            eprintln!("yosh: getopts: usage: getopts optstring name [arg ...]");
            return Ok(2);
        }
        Err(ArgError::InvalidVarName(name)) => {
            eprintln!("yosh: getopts: `{}': not a valid identifier", name);
            return Ok(2);
        }
    };

    let silent = parsed.optstring.starts_with(':');
    let spec = if silent { &parsed.optstring[1..] } else { parsed.optstring };

    // Resolve operands: explicit args[2..] if non-empty, else positional params.
    let positional_owned: Vec<String>;
    let operands_refs: Vec<&str> = if parsed.operands.is_empty() {
        positional_owned = env.vars.positional_params().to_vec();
        positional_owned.iter().map(String::as_str).collect()
    } else {
        parsed.operands.clone()
    };

    // Read OPTIND from env (fallback to 1). If it reads as 1, reset
    // the subcursor so a user-written `OPTIND=1` re-starts parsing.
    let optind_in = env
        .vars
        .get("OPTIND")
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1);
    if optind_in == 1 {
        env.vars.set_getopts_subindex(0);
    }
    let subindex_in = env.vars.getopts_subindex();

    let step = step_getopts(spec, &operands_refs, optind_in, subindex_in, silent);

    // Apply
    let _ = env.assign_var(parsed.var_name, step.var_value);
    let optarg_value = step.optarg.unwrap_or_default();
    let _ = env.assign_var("OPTARG", optarg_value);
    let _ = env.assign_var("OPTIND", step.optind.to_string());
    env.vars.set_getopts_subindex(step.subindex);

    if let Some(msg) = step.stderr {
        eprintln!("yosh: getopts: {}", msg);
    }

    Ok(step.exit)
}
```

- [ ] **Step 4: Run getopts module tests**

Run: `cargo test -p yosh --lib builtin::getopts`
Expected: PASS — 17 step + parse tests + 8 new builtin_ tests = 25 tests.

- [ ] **Step 5: Wire `getopts` into the dispatch table**

Edit `src/builtin/mod.rs:13-19` (BUILTIN_NAMES) — append `"getopts"`:

```rust
pub const BUILTIN_NAMES: &[&str] = &[
    // Special builtins
    "break", ":", "continue", ".", "eval", "exec", "exit", "export", "readonly", "return", "set",
    "shift", "times", "trap", "unset", "fc", // Regular builtins
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[", "type", "hash", "read", "getopts",
];
```

Edit `src/builtin/mod.rs:34-42` (`classify_builtin`) — add `"getopts"` to the Regular arm:

```rust
pub fn classify_builtin(name: &str) -> BuiltinKind {
    match name {
        "break" | ":" | "continue" | "." | "eval" | "exec" | "exit" | "export" | "readonly"
        | "return" | "set" | "shift" | "times" | "trap" | "unset" | "fc" => BuiltinKind::Special,
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "test" | "[" | "type" | "hash" | "read" | "getopts" => BuiltinKind::Regular,
        _ => BuiltinKind::NotBuiltin,
    }
}
```

Edit `src/builtin/mod.rs:73` — add `"getopts"` arm to `exec_regular_builtin` right after the existing `"read"` arm:

```rust
        "read" => read::builtin_read(args, env),
        "getopts" => getopts::builtin_getopts(args, env),
```

- [ ] **Step 6: Add a classify_builtin test for getopts**

Append in the `#[cfg(test)] mod tests` block of `src/builtin/mod.rs`, in `test_classify_builtin`:

```rust
        assert!(matches!(classify_builtin("getopts"), BuiltinKind::Regular));
```

- [ ] **Step 7: Run all builtin tests**

Run: `cargo test -p yosh --lib builtin`
Expected: PASS — getopts dispatched as Regular, all existing builtin classification/dispatch tests stay green.

- [ ] **Step 8: Run full lib + integration tests**

Run: `cargo test -p yosh`
Expected: PASS — including `tests/*.rs` integration tests.

- [ ] **Step 9: Commit**

```bash
git add src/builtin/getopts.rs src/builtin/mod.rs
git commit -m "$(cat <<'EOF'
feat(builtin): native getopts builtin (POSIX XCU)

`getopts optstring var [arg ...]` parses one option per call from
positional parameters (or explicit operands), advances OPTIND, sets
OPTARG, supports stacked options (-abc), `--` terminator, silent
mode (`:optstring`), and unknown/missing-argument diagnostics.

Dispatches as a Regular builtin via `src/builtin/mod.rs`; the pure
`step_getopts` core enables direct unit testing without a ShellEnv.
Subcursor + saved-OPTIND infrastructure landed in earlier commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Group G3 — E2E XFAIL unblock

### Task 7: Remove `# XFAIL:` headers from all 9 SP4 test files

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/getopts_basic.sh`
- Modify: `e2e/posix_spec/4_required_builtin/getopts_with_arg.sh`
- Modify: `e2e/posix_spec/4_required_builtin/getopts_stacked.sh`
- Modify: `e2e/posix_spec/4_required_builtin/getopts_unknown.sh`
- Modify: `e2e/posix_spec/4_required_builtin/getopts_missing_arg.sh`
- Modify: `e2e/posix_spec/4_required_builtin/getopts_optind.sh`
- Modify: `e2e/posix_spec/8_env_vars/OPTIND_initial_one.sh`
- Modify: `e2e/posix_spec/8_env_vars/OPTIND_advances.sh`
- Modify: `e2e/posix_spec/8_env_vars/OPTARG_set_by_getopts.sh`

- [ ] **Step 1: Build the latest binary**

Run: `cargo build`
Expected: success — required because the e2e runner does not auto-rebuild.

- [ ] **Step 2: Confirm the 9 tests currently report XFAIL**

Run: `./e2e/run_tests.sh --filter=getopts`
Expected: getopts tests show `XFAIL → PASS` (or similar transition marker) because the implementation now exists. If runner still reports `XFAIL` because the headers are present, that confirms the strip is needed in Step 3.

Run: `./e2e/run_tests.sh --filter=OPT`
Expected: OPTIND/OPTARG tests show similar transition signal.

- [ ] **Step 3: Strip the XFAIL line from each file**

For each file, use the `Edit` tool to delete its `# XFAIL: ...` line. Example for the first file (`getopts_basic.sh`):

```
old_string:
# DESCRIPTION: getopts a opt parses -a from $@
# XFAIL: not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: a

new_string:
# DESCRIPTION: getopts a opt parses -a from $@
# EXPECT_OUTPUT: a
```

Apply the same pattern for each of the other 8 files. The exact XFAIL strings are:

- `getopts_basic.sh`: `# XFAIL: not yet implemented (TODO: implement getopts)`
- `getopts_with_arg.sh`: `# XFAIL: not yet implemented (TODO: implement getopts)`
- `getopts_stacked.sh`: `# XFAIL: not yet implemented (TODO: implement getopts)`
- `getopts_unknown.sh`: `# XFAIL: not yet implemented (TODO: implement getopts)`
- `getopts_missing_arg.sh`: `# XFAIL: not yet implemented (TODO: implement getopts)`
- `getopts_optind.sh`: `# XFAIL: not yet implemented (TODO: implement getopts)`
- `OPTIND_initial_one.sh`: `# XFAIL: not yet implemented (TODO: implement getopts; OPTIND default-init requires native getopts)`
- `OPTIND_advances.sh`: `# XFAIL: not yet implemented (TODO: implement getopts; OPTIND advance requires native getopts)`
- `OPTARG_set_by_getopts.sh`: `# XFAIL: not yet implemented (TODO: implement getopts; OPTARG is set by getopts)`

- [ ] **Step 4: Run filtered E2E to confirm PASS**

Run: `./e2e/run_tests.sh --filter=getopts`
Expected: All 8 getopts tests (6 SP4 + the previously-passing `getopts_no_more` and `getopts_end_with_double_dash`) → PASS, no XFAIL.

Run: `./e2e/run_tests.sh --filter=OPT`
Expected: All 3 OPT* tests → PASS.

- [ ] **Step 5: Run full E2E suite and verify XFail count drops to 21**

Run: `./e2e/run_tests.sh`
Expected: Summary line includes `XFail: 21` (was 30). No PASS regressions; no new FAIL entries.

- [ ] **Step 6: Run cargo test for sanity**

Run: `cargo test -p yosh`
Expected: PASS — confirming no integration test depends on the old behavior.

- [ ] **Step 7: Verify file permissions remain 644**

Run: `find e2e/posix_spec/4_required_builtin/getopts_*.sh e2e/posix_spec/8_env_vars/OPT*.sh -perm 755`
Expected: No output. (Per CLAUDE.md: "E2E test files should have 644 permissions, not 755.") If any file is 755, run `chmod 644 <file>`.

- [ ] **Step 8: Commit**

```bash
git add e2e/posix_spec/4_required_builtin/getopts_basic.sh \
        e2e/posix_spec/4_required_builtin/getopts_with_arg.sh \
        e2e/posix_spec/4_required_builtin/getopts_stacked.sh \
        e2e/posix_spec/4_required_builtin/getopts_unknown.sh \
        e2e/posix_spec/4_required_builtin/getopts_missing_arg.sh \
        e2e/posix_spec/4_required_builtin/getopts_optind.sh \
        e2e/posix_spec/8_env_vars/OPTIND_initial_one.sh \
        e2e/posix_spec/8_env_vars/OPTIND_advances.sh \
        e2e/posix_spec/8_env_vars/OPTARG_set_by_getopts.sh

git commit -m "$(cat <<'EOF'
test(e2e): drop XFAIL from 9 SP4 getopts/OPT* tests

Native `getopts` builtin (added in this branch) now passes all six
4_required_builtin/getopts_* tests plus the three 8_env_vars/OPT*
tests. E2E XFail count: 30 → 21.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Group G4 — Roadmap closure

### Task 8: Delete completed entries from TODO.md and update memory

**Files:**
- Modify: `TODO.md`
- Modify: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`

- [ ] **Step 1: Delete the SP4 roadmap bullet in TODO.md**

In `TODO.md`, the `## E2E XFAIL Roadmap` section currently includes:

```
- [ ] SP4 — `getopts` builtin implementation (9 tests)
```

Delete that one line.

- [ ] **Step 2: Delete the `getopts` POSIX-required-builtin bullet in TODO.md**

In `TODO.md` under `## Future: POSIX Required Builtin Implementation`, delete:

```
- [ ] `getopts optstring var [args]` — option-parsing helper, used in
      portable shell scripts. Currently uses `/usr/bin/getopts`. XFAIL
      tests: `e2e/posix_spec/4_required_builtin/getopts_*.sh` (6 of 8 tests
      pass via fallback; 6 remain XFAIL pending native impl)
```

- [ ] **Step 3: Update auto-memory roadmap status**

In `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`:

- Change the `description:` frontmatter line to:
  `description: "55-XFAIL-test decomposition roadmap; SP1+SP2+SP3+SP4 complete (2026-05-14), 21 XFails remain across SP5-SP7"`
- Change the **Status** heading text from `**Status (as of 2026-05-14):**` to `**Status (as of 2026-05-14):**` (date unchanged if same calendar day) and insert a new SP4 line after the SP3 line:
  ```
  - **SP4 COMPLETE** (2026-05-14): 9 tests — native `getopts` builtin with stacked options, silent mode, `--` terminator, and per-function OPTIND save/restore. Spec `2026-05-14-e2e-xfail-sp4-getopts-design.md`. Plan `2026-05-14-e2e-xfail-sp4-getopts.md`. Follow-ups (if any) under `### SP4 follow-ups (non-blocking)` in TODO.md.
  ```
- Replace the `- **SP4 pending**: 9 tests — \`getopts\` builtin implementation. Spec needed.` line with nothing (delete it).
- Change the closing summary line `After SP1+SP2+SP3: 55 - 11 - 5 - 9 = 30 XFails remain (matches \`./e2e/run_tests.sh\` baseline output \`XFail: 30\`).` to:
  `After SP1+SP2+SP3+SP4: 55 - 11 - 5 - 9 - 9 = 21 XFails remain (matches \`./e2e/run_tests.sh\` baseline output \`XFail: 21\`).`

- [ ] **Step 4: Update MEMORY.md index entry hook text**

In `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/MEMORY.md`, change:

```
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1+SP2+SP3 COMPLETE (2026-05-14, 30 XFails remain); SP4-SP7 pending
```

to:

```
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1+SP2+SP3+SP4 COMPLETE (2026-05-14, 21 XFails remain); SP5-SP7 pending
```

- [ ] **Step 5: Final verification**

Run: `cargo test -p yosh && ./e2e/run_tests.sh 2>&1 | tail -5`
Expected: `cargo test` green, e2e summary shows `XFail: 21`.

- [ ] **Step 6: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
chore(sp4): close SP4 — remove roadmap entry and getopts TODO bullet

SP4 (native getopts builtin, 9 XFAIL → PASS) is complete. Following
the project convention "delete completed items from TODO.md", remove
both the SP4 roadmap line and the getopts entry under POSIX Required
Builtin Implementation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

(The memory file updates are not committed to git — they live outside the repo in `~/.claude/projects/...`.)

---

## Acceptance Criteria Recap

1. `cargo test -p yosh` green (lib + integration).
2. `./e2e/run_tests.sh` reports `XFail: 21`.
3. All 9 target XFAIL files no longer carry `# XFAIL:` headers.
4. `getopts_no_more.sh` and `getopts_end_with_double_dash.sh` remain PASS.
5. `TODO.md`: SP4 roadmap line + `getopts` POSIX-required-builtin bullet both deleted.
6. Auto-memory `project_e2e_xfail_roadmap.md` + `MEMORY.md` reflect SP4 completion.
