# SP2 — Required-builtin diagnostics + native `type` / `hash`

**Date:** 2026-05-14
**Roadmap:** `docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md`
**Status:** Design (implementation plan TBD via writing-plans skill)
**Scope:** 5 XFAIL tests in `e2e/posix_spec/4_required_builtin/`.

## 1. Background

The E2E XFAIL roadmap partitions 55 XFAIL tests into seven sub-projects.
SP2 — the second sub-project — closes five XFAILs by:

1. fixing `jobs` so unknown options and unknown job specs return exit 1
   with a diagnostic instead of being silently ignored, and
2. replacing the `/usr/bin/type` and `/usr/bin/hash` fallthrough paths
   with native builtins that can see yosh's session aliases, functions,
   and (for `hash`) a real per-shell utility-location cache.

The five tests already declare the correct
`EXPECT_OUTPUT`/`EXPECT_EXIT`/`EXPECT_STDERR`; closing SP2 means
removing the `# XFAIL: …` line so each test runs as a normal
expectation.

## 2. Tests in scope (5)

| Group | Test | Current | Expected |
|-------|------|---------|----------|
| G1 | `4_required_builtin/jobs_unknown_spec.sh` | `jobs %99 >/dev/null` exits 0, no stderr | exit 1, stderr contains `jobs` |
| G1 | `4_required_builtin/jobs_invalid_option.sh` | `jobs -x >/dev/null` exits 0, no stderr | exit 1, stderr contains `jobs` |
| G2 | `4_required_builtin/type_alias.sh` | `/usr/bin/type` cannot see yosh aliases — stdout omits `alias` | stdout contains `alias`, exit 0 |
| G2 | `4_required_builtin/type_function.sh` | `/usr/bin/type` cannot see yosh functions — stdout omits `function` | stdout contains `function`, exit 0 |
| G3 | `4_required_builtin/hash_unknown_cmd.sh` | `/usr/bin/hash /no/such/cmd_$$` exits 0 | exit 1 |

Three additional `type_*.sh` tests and three additional `hash_*.sh`
tests currently PASS via the external-binary fallthrough. Replacing
the fallthrough with native builtins MUST keep these tests passing —
they form the regression gate for SP2.

## 3. Approach

Implement in three independent groups, in this commit order:

1. **G1 — `jobs` option/spec validation** (state-free, smallest)
2. **G2 — native `type`** (state-free, reuses `resolve_command_kind`)
3. **G3 — native `hash` + utility-hash cache + PATH invalidation**
   (introduces new `ShellEnv` state and rewires `lookup_in_path`)

Each group lands as its own commit so review and revert are localized.
This mirrors the SP1 group-by-group rollout.

## 4. Design by group

### 4.1 G1 — `jobs` diagnostics

POSIX (XCU §1.4 `jobs`): unknown option and unknown job spec MUST
produce a non-zero exit and a diagnostic. Today `builtin_jobs` accepts
any `-X` and silently iterates only known flags, and unknown specs
(`%99`) are ignored.

**Changes:**

- `src/exec/job_control.rs::builtin_jobs`:
  - Introduce a private `parse_options(args: &[String]) -> Result<JobsOpts, String>`
    helper that walks `args` while the next item starts with `-` (and
    is not `--` or a bare `-`), recognizes `-l` / `-p` (clustered forms
    like `-lp` / `-pl` allowed), and returns
    `Err("jobs: -X: invalid option")` on any other flag character. On
    `--`, stop flag scanning and treat the rest as operands.
  - When `parse_options` returns `Err(msg)`, write
    `yosh: {msg}` to stderr and return `Ok(1)`.
  - For each positional operand (job spec), call
    `self.env.process.jobs.resolve_job_spec(spec)`. Map
    `JobSpecError::Malformed` and `JobSpecError::NoSuchJob` to
    `yosh: jobs: {spec}: no such job` (status 1, continue iterating);
    `JobSpecError::Ambiguous` to
    `yosh: jobs: {display}: ambiguous job spec` where `display` strips
    the `%` prefix (same convention used in `builtin_fg`).
  - If `args` contained operands but no resolutions succeeded — exit
    1. If at least one operand resolved and emitted output, accumulate
    the worst exit (1 if any operand failed, 0 only when all succeeded
    or there were no operands).
- `parse_options` is unit-tested for `-l`, `-p`, `-lp`, `-pl`, `--`,
  and `-x` (invalid option). The integration cases (`jobs %99`,
  `jobs -x`) are covered by the E2E tests.

**Rationale for keeping `parse_options` local to `job_control.rs`:**
The `-l`/`-p` flags are unique to `jobs`; no other builtin reuses
them. SP1 §G2's per-builtin parsing pattern (export/readonly/unset
each parse their own flags) is the established style.

### 4.2 G2 — native `type` builtin

POSIX (XCU §1.4 `type`): `type name...` displays each name as it
would be interpreted as a command (alias / function / builtin /
external / not found). Multiple operands; exit 0 only when every
operand resolves.

**File layout:**

- New file `src/builtin/type.rs` (raw identifier because `type` is a
  Rust keyword) following the `src/builtin/command.rs` precedent for
  per-builtin files.
- `src/builtin/mod.rs`:
  - Append `"type"` to `BUILTIN_NAMES`.
  - In `classify_builtin`, add `"type"` to the `Regular` arm.
  - In `exec_regular_builtin`, dispatch `"type"` to
    `r#type::builtin_type(args, env)`.

**API:**

```rust
pub fn builtin_type(args: &[String], env: &ShellEnv) -> Result<i32, ShellError>;

// Internal pure helper, unit-testable.
fn format_type_line(env: &ShellEnv, name: &str) -> (String, Option<String>, i32);
//   Returns (stdout_line, optional_stderr_line, per_operand_exit).
//   stderr_line is Some only on NotFound; per_operand_exit is 1 only on NotFound.
```

`builtin_type` reuses `crate::builtin::resolve::resolve_command_kind`
to classify each name. The output mapping (see table below) is
written directly in `format_type_line` rather than calling
`crate::builtin::command::render_verbose`, so a future divergence
between `command -V` and `type` (e.g., `type -t` short form) can be
made without disturbing `command`.

**Output format:**

| CommandKind | stdout |
|---|---|
| `Alias(value)` | `<name> is aliased to '<escaped_value>'` (single-quote → `'\''`) |
| `Keyword` | `<name> is a shell keyword` |
| `Function` | `<name> is a function` |
| `Builtin(Special)` | `<name> is a special shell builtin` |
| `Builtin(Regular)` | `<name> is a shell builtin` |
| `External(path)` | `<name> is <path>` |
| `NotFound` | (stdout empty); stderr: `yosh: type: <name>: not found`; exit 1 |

These match bash/dash conventions. Substring checks `grep -q alias` /
`grep -q function` / `grep -q builtin` pass against the corresponding
rows.

**Argument handling:**

- Zero operands → `eprintln!("yosh: type: usage: type name...")`,
  return `Ok(2)` (usage error).
- One or more operands → loop, accumulate exit (1 if any operand
  NotFound, 0 only when all resolve).
- No flag support in scope (POSIX `type` takes no options;
  bash-specific `-a`/`-t`/`-p` are deferred — note in TODO.md if a
  consumer asks).

**Tests:**

- E2E: remove `# XFAIL: …` from `type_alias.sh` and `type_function.sh`.
- Unit (`r#type.rs::tests`): one case each for Alias / Function /
  Builtin(Special) / Builtin(Regular) / Keyword / External / NotFound,
  plus a multi-operand mixed case (one found + one not found → exit 1
  with both lines emitted).
- Regression: `type_builtin.sh`, `type_external.sh`,
  `type_not_found.sh` must keep passing.

### 4.3 G3 — native `hash` builtin + utility-hash cache

POSIX (XCU §1.4 `hash`, §2.5.3 special parameters): `hash` maintains
a per-shell command-location cache. `hash` with no operands lists the
cache; `hash -r` clears it; `hash utility...` records the location of
each utility (errors if the utility cannot be located in `PATH`).
Modifying `PATH` invalidates the cache. Empty cache + bare `hash` is
not an error.

This group has three pieces: (a) `ShellEnv` state, (b) `PATH`
invalidation plumbing, (c) the `hash` builtin itself plus
`lookup_in_path` integration.

#### 4.3.1 ShellEnv state

`src/env/mod.rs`:

```rust
pub struct ShellEnv {
    // … existing fields …
    /// POSIX hash table: utility name → resolved absolute path.
    /// Auto-populated by `lookup_in_path` cache misses and by
    /// explicit `hash utility...` invocations. Cleared by `hash -r`
    /// and on `PATH` reassignment (POSIX §2.5.3).
    pub utility_hash: HashMap<String, PathBuf>,
}
```

Initialized to `HashMap::new()` in `ShellEnv::new`. `Clone` continues
to derive (subshell copies inherit the cache — POSIX leaves this
implementation-defined and matches bash).

#### 4.3.2 PATH invalidation helpers

Add two helpers on `impl ShellEnv`:

```rust
pub fn assign_var(&mut self, name: &str, value: impl Into<String>)
    -> Result<(), String>
{
    self.vars.set(name, value)?;
    if name == "PATH" {
        self.utility_hash.clear();
    }
    Ok(())
}

pub fn unset_var(&mut self, name: &str) -> Result<(), String> {
    self.vars.unset(name)?;
    if name == "PATH" {
        self.utility_hash.clear();
    }
    Ok(())
}
```

`VarStore::set` returns `Result<(), String>` (readonly-violation
case); `VarStore::unset` likewise returns `Result<(), String>`. The
helpers preserve the existing error type. Cache clearing happens only
on successful `name == "PATH"` mutation (the `?` short-circuits on
error before `clear()` runs).

**Call sites to migrate** (full audit via `grep -rn 'vars\.set(' src/`
and `grep -rn 'vars\.unset(' src/`):

- `src/builtin/special.rs::builtin_export` — variable assignment path
- `src/builtin/special.rs::builtin_readonly` — assignment path
- `src/builtin/special.rs::builtin_unset` — switches to
  `env.unset_var(name)`
- `src/exec/simple.rs` — prefix-assignment path and assignment-only
  simple command path (any `env.vars.set` invocation that handles
  runtime user-supplied variable names)

**Out of scope** (no PATH risk):

- `VarStore::from_environ()` — startup, before any cache entries exist
- `VarStore::set_positional_params` — sets `$#`, `$1`, etc., not PATH
- Internal helpers that set non-user variables (`$?`, `$LINENO`, etc.)

The migration is mechanical but must be exhaustive. The acceptance
gate is a `grep -rn 'env\.vars\.set("PATH"' src/` returning zero hits
(production code) after the change.

#### 4.3.3 `lookup_in_path` integration

`src/exec/command.rs::lookup_in_path` and `find_in_path` today are
pure functions that take `(cmd: &str, path_var: &str)`. Extend the
signatures to accept a mutable cache reference:

```rust
pub fn lookup_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut HashMap<String, PathBuf>,
) -> PathLookup;

pub fn find_in_path(
    cmd: &str,
    path_var: &str,
    cache: &mut HashMap<String, PathBuf>,
) -> Option<PathBuf>;
```

Behavior:

- If `cmd` contains a `/`, skip the cache entirely (POSIX: pathnames
  with `/` bypass PATH search), and do not record the result.
- Otherwise, if `cache.get(cmd)` returns `Some(path)` AND
  `path.is_file()` AND `path` is executable (`access(X_OK)`), return
  that path immediately (POSIX: hashed location is used directly).
- On cache miss or stale entry (entry exists but file no longer
  executable), fall through to the existing PATH walk. If the walk
  succeeds, `cache.insert(cmd.to_string(), path.clone())` (auto-hash).
- On total miss (PATH walk also fails), do not touch the cache; return
  `PathLookup::NotFound` / `None` as before.

**Call site updates:** every caller of `lookup_in_path` /
`find_in_path` must pass `&mut env.utility_hash`. The full call set
(per `grep -rn 'lookup_in_path\|find_in_path' src/`):

- `src/exec/command.rs` tests (use a local empty `HashMap`)
- `src/exec/simple.rs` (executor command resolution)
- `src/builtin/command.rs` (`command -v` / `-V` resolution)
- `src/builtin/resolve.rs::resolve_command_kind` (signature already
  takes `&ShellEnv`; thread `cache` through internally — note this
  function will need to migrate to `&mut ShellEnv` OR a dedicated
  `&mut HashMap` parameter)

**Trade-off note for `resolve_command_kind`:** changing its signature
from `(&ShellEnv, &str)` to `(&mut ShellEnv, &str)` ripples through
`render_brief`/`render_verbose`/`builtin_type` (all currently `&ShellEnv`).
Alternative: thread `&mut HashMap` as a separate argument. Pick the
threaded-cache variant — it keeps the existing functions immutable
over `ShellEnv` (matters for unit tests that build a snapshot) and
makes the cache dependency explicit at the call site.

#### 4.3.4 `hash` builtin

New file `src/builtin/hash.rs`:

```rust
pub fn builtin_hash(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError>;
```

**Flag parsing:** Walk leading `-X` args. Recognize `-r` only. `--`
terminates flag scanning. Any other `-X` →
`eprintln!("yosh: hash: -{}: invalid option", c)`, return `Ok(1)`.

**Modes:**

- **List (no operands, no `-r`):** collect `env.utility_hash` keys into
  a `Vec`, sort lexicographically for deterministic output, print one
  entry per sorted name as `<path>`. (`hash_no_arg_lists.sh` checks
  exit 0 only; bash also prints `hits=N\tname` but no test requires
  that format, so we keep the simpler `<path>` form.) Empty cache → no
  output. Exit 0.
- **`-r` with no operands:** `env.utility_hash.clear()`, exit 0.
- **`-r` with operands:** POSIX allows `-r` plus operands (some shells
  reject). For SP2, accept the combination: clear first, then process
  operands. Note: bash rejects this. We follow dash, which accepts.
  (If a future test demands the bash behavior, gate `-r` to exclusive.)
- **Operands only:** for each operand:
  - If operand contains `/`: treat as explicit path.
    `PathBuf::from(operand)`. If `path.is_file()` AND
    `access(X_OK).is_ok()`, `env.utility_hash.insert(name, path)`
    where `name` is the operand basename (POSIX: the cached name is
    what the shell will see). On failure, stderr:
    `yosh: hash: {operand}: not found`, status 1.
  - Otherwise: call
    `find_in_path(operand, &path_var, &mut env.utility_hash)`. On
    `Some(p)`, the helper already inserted into the cache. On `None`,
    stderr: `yosh: hash: {operand}: not found`, status 1.
- Accumulate exit (1 if any operand failed, 0 otherwise).

**`mod.rs` wiring** (same shape as G2):

- Append `"hash"` to `BUILTIN_NAMES`.
- `classify_builtin`: `"hash"` in the `Regular` arm.
- `exec_regular_builtin`: dispatch `"hash"` to
  `hash::builtin_hash(args, env)`.

#### 4.3.5 Tests

- **E2E**: remove `# XFAIL: …` from `hash_unknown_cmd.sh`. The three
  passing `hash_*.sh` tests are the regression gate.
- **Unit (`hash.rs::tests`):**
  - `hash -r` clears a pre-populated cache.
  - `hash /no/such/path` returns 1 with `not found` stderr; cache
    unchanged.
  - `hash echo` succeeds (PATH lookup), cache contains `echo`.
  - `hash` no-arg with empty cache → exit 0, no output.
  - `hash` no-arg with populated cache → exit 0, output contains all
    cached paths.
  - Invalid option `-x` → exit 1, stderr contains `invalid option`.
- **Unit (`src/env/mod.rs::tests`):**
  - `env.assign_var("PATH", "/new")` clears `utility_hash`.
  - `env.assign_var("FOO", "bar")` leaves `utility_hash` unchanged.
  - `env.unset_var("PATH")` clears `utility_hash`.
- **Unit (`src/exec/command.rs::tests`):**
  - Cache hit returns the cached path without re-walking PATH.
  - Cache hit but file missing falls back to PATH walk.
  - Auto-hash: PATH-walk success inserts into cache.
  - `/`-containing cmd bypasses cache entirely.

## 5. Cross-cutting

### 5.1 Files touched (summary)

| Group | Files |
|---|---|
| G1 | `src/exec/job_control.rs`; 2 E2E test files (XFAIL removal) |
| G2 | `src/builtin/type.rs` (new); `src/builtin/mod.rs`; 2 E2E test files |
| G3 | `src/env/mod.rs`; `src/exec/command.rs`; `src/builtin/hash.rs` (new); `src/builtin/mod.rs`; `src/builtin/special.rs`; `src/builtin/resolve.rs`; `src/exec/simple.rs`; `src/builtin/command.rs`; 1 E2E test file |
| All | `TODO.md`, `docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md` (status note if convention requires) |

### 5.2 Commit plan

1. `fix(builtin): jobs validates options and job specs` — G1
2. `feat(builtin): native type builtin` — G2
3. `feat(builtin): native hash with PATH cache invalidation` — G3
4. `chore(sp2): close SP2 — remove roadmap entry` — TODO.md SP2 line +
   `cargo fmt` if any drift

Each commit MUST keep `cargo test` and `./e2e/run_tests.sh` green so
bisect-driven debugging works.

### 5.3 TODO.md updates

When SP2 closes:

- Delete the SP2 line under `## E2E XFAIL Roadmap`.
- Delete the `type name...` entry under
  `## Future: POSIX Required Builtin Implementation`.
- Delete the `hash [-r] [cmd]` entry under the same section.
- Delete the `jobs returns exit 0 for an unknown job spec…` entry
  under `## Future: POSIX Conformance Bugs`.

`getopts`, `read`, `ulimit` entries remain (SP3 / SP4 / SP7).

### 5.4 Acceptance

SP2 is complete when:

- All 5 listed E2E tests PASS under `./e2e/run_tests.sh` with their
  `# XFAIL: …` lines removed.
- `cargo test` green.
- `cargo fmt --all -- --check` clean.
- `cargo clippy --all-targets -- -D warnings` clean (excluding the
  pre-existing `src/plugin/mod.rs:98-99` umbrella exception).
- No regression: the six currently-passing `type_*.sh` / `hash_*.sh`
  tests still PASS; all `jobs %1`-style PTY tests still PASS.
- `grep -rn 'env\.vars\.set("PATH"' src/` returns no hits (PATH
  mutations all routed through `env.assign_var`).
- `TODO.md` updated per §5.3.

## 6. Out of scope

- bash-specific `type -a` / `-t` / `-p` flags (note as TODO.md item if
  a consumer asks).
- `hash` hit-count display (POSIX optional, bash shows `hits=N`; yosh
  prints paths only).
- `set -h` / `set +h` (hash on/off toggle).
- Native `getopts`, `read`, `ulimit` — covered by SP3 / SP4 / SP7.

## 7. Open questions

None at design time. Implementation plan (via writing-plans) will
surface any low-level decisions discovered during coding.
