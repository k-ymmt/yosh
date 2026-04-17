# Job Spec String Matching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement POSIX `%string` / `%?string` job specifier matching with bash-compatible `"no such job"` vs `"ambiguous job spec"` error messages.

**Architecture:** Introduce a `JobSpec<'a>` enum and pure `parse_job_spec` function in `src/env/jobs.rs`. Reshape `JobTable::resolve_job_spec` from `Option<JobId>` to `Result<JobId, JobSpecError>` so callers (`fg`, `bg`, `wait`) can distinguish "no match" from "ambiguous match". Matching is performed against `Job.command` (full command line) across all job statuses, bash-compatible.

**Tech Stack:** Rust edition 2024, `nix` crate for `Pid`, Rust stdlib `HashMap`, existing `ShellError` / `RuntimeErrorKind` types.

**Spec:** `docs/superpowers/specs/2026-04-17-job-spec-string-matching-design.md`

---

## Task 1: Add JobSpec types, parser, and resolver (pure addition)

**Files:**
- Modify: `src/env/jobs.rs` (top of file + top of `impl JobTable` + tests module)

This task introduces the new types, parser, and `JobTable::resolve` method without touching the existing `resolve_job_spec(&str) -> Option<JobId>` API. Nothing outside `jobs.rs` changes, so compilation stays green.

- [ ] **Step 1: Add test cases for `parse_job_spec`**

Add at the bottom of the `#[cfg(test)] mod tests` block in `src/env/jobs.rs` (after the existing `resolve_job_spec` tests):

```rust
// -----------------------------------------------------------------------
// parse_job_spec
// -----------------------------------------------------------------------

#[test]
fn test_parse_current_percent() {
    assert_eq!(parse_job_spec("%%"), Ok(JobSpec::Current));
}

#[test]
fn test_parse_current_plus() {
    assert_eq!(parse_job_spec("%+"), Ok(JobSpec::Current));
}

#[test]
fn test_parse_previous() {
    assert_eq!(parse_job_spec("%-"), Ok(JobSpec::Previous));
}

#[test]
fn test_parse_numeric() {
    assert_eq!(parse_job_spec("%1"), Ok(JobSpec::Numeric(1)));
    assert_eq!(parse_job_spec("%42"), Ok(JobSpec::Numeric(42)));
}

#[test]
fn test_parse_numeric_overflow() {
    assert_eq!(parse_job_spec("%99999999999999999999"), Err(JobSpecError::Malformed));
}

#[test]
fn test_parse_prefix() {
    assert_eq!(parse_job_spec("%foo"), Ok(JobSpec::Prefix("foo")));
    assert_eq!(parse_job_spec("%vim"), Ok(JobSpec::Prefix("vim")));
}

#[test]
fn test_parse_substring() {
    assert_eq!(parse_job_spec("%?bar"), Ok(JobSpec::Substring("bar")));
    assert_eq!(parse_job_spec("%?READ"), Ok(JobSpec::Substring("READ")));
}

#[test]
fn test_parse_prefix_hyphen() {
    // "%-foo" is NOT %- followed by "foo" — it is a Prefix("-foo")
    assert_eq!(parse_job_spec("%-foo"), Ok(JobSpec::Prefix("-foo")));
}

#[test]
fn test_parse_prefix_double_percent() {
    // "%%foo" is NOT Current followed by "foo" — it is Prefix("%foo")
    assert_eq!(parse_job_spec("%%foo"), Ok(JobSpec::Prefix("%foo")));
}

#[test]
fn test_parse_malformed_empty() {
    assert_eq!(parse_job_spec(""), Err(JobSpecError::Malformed));
}

#[test]
fn test_parse_malformed_bare_percent() {
    assert_eq!(parse_job_spec("%"), Err(JobSpecError::Malformed));
}

#[test]
fn test_parse_malformed_bare_question() {
    assert_eq!(parse_job_spec("%?"), Err(JobSpecError::Malformed));
}

#[test]
fn test_parse_malformed_no_percent() {
    assert_eq!(parse_job_spec("foo"), Err(JobSpecError::Malformed));
    assert_eq!(parse_job_spec("1"), Err(JobSpecError::Malformed));
}
```

- [ ] **Step 2: Run parser tests — verify failure**

Run: `cargo test --lib env::jobs::tests::test_parse -- --nocapture`
Expected: compile errors (`JobSpec`, `JobSpecError`, `parse_job_spec` not found).

- [ ] **Step 3: Add `JobSpec` and `JobSpecError` enums**

At the top of `src/env/jobs.rs`, after the `Job` struct (around line 34, before `impl JobTable`):

```rust
// ---------------------------------------------------------------------------
// JobSpec (POSIX §3.204 Job Control Job ID)
// ---------------------------------------------------------------------------

/// Parsed form of a POSIX job specifier string such as `%%`, `%1`, `%vim`.
///
/// Borrows from the input string so parsing is zero-allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpec<'a> {
    /// `%%` or `%+` — current job
    Current,
    /// `%-` — previous job
    Previous,
    /// `%n` — job with numeric id
    Numeric(JobId),
    /// `%string` — command begins with string
    Prefix(&'a str),
    /// `%?string` — command contains string
    Substring(&'a str),
}

/// Error returned by `parse_job_spec` and `JobTable::resolve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpecError {
    /// Input is not a syntactically valid job specifier.
    Malformed,
    /// Parse succeeded but no job matches the spec.
    NoSuchJob,
    /// A Prefix or Substring spec matched two or more jobs.
    Ambiguous,
}
```

- [ ] **Step 4: Add `parse_job_spec` free function**

Just below the `JobSpecError` enum, add:

```rust
/// Parse a POSIX job specifier string into a `JobSpec`.
///
/// Disambiguation order (earliest match wins):
/// 1. `"%%"` / `"%+"` → `Current`
/// 2. `"%-"` → `Previous`
/// 3. `"%<digits>"` with non-empty digit run → `Numeric`
/// 4. `"%?<rest>"` with non-empty `rest` → `Substring`
/// 5. `"%<rest>"` with non-empty `rest` → `Prefix`
/// 6. Otherwise → `Malformed`
pub fn parse_job_spec(s: &str) -> Result<JobSpec<'_>, JobSpecError> {
    let rest = s.strip_prefix('%').ok_or(JobSpecError::Malformed)?;

    match rest {
        "" => Err(JobSpecError::Malformed),
        "%" | "+" => Ok(JobSpec::Current),
        "-" => Ok(JobSpec::Previous),
        _ => {
            // Pure digit run → Numeric
            if rest.bytes().all(|b| b.is_ascii_digit()) {
                return rest
                    .parse::<JobId>()
                    .map(JobSpec::Numeric)
                    .map_err(|_| JobSpecError::Malformed);
            }

            // "?<rest>" → Substring
            if let Some(sub) = rest.strip_prefix('?') {
                if sub.is_empty() {
                    return Err(JobSpecError::Malformed);
                }
                return Ok(JobSpec::Substring(sub));
            }

            // Everything else with non-empty rest → Prefix
            Ok(JobSpec::Prefix(rest))
        }
    }
}
```

- [ ] **Step 5: Run parser tests — verify success**

Run: `cargo test --lib env::jobs::tests::test_parse`
Expected: all 14 parser tests pass.

- [ ] **Step 6: Add test cases for `JobTable::resolve`**

Append to the tests module in `src/env/jobs.rs`, after the parser tests:

```rust
// -----------------------------------------------------------------------
// JobTable::resolve
// -----------------------------------------------------------------------

#[test]
fn test_resolve_current() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "x", false);
    assert_eq!(table.resolve(JobSpec::Current), Ok(id));
}

#[test]
fn test_resolve_current_unset() {
    let table = JobTable::default();
    assert_eq!(table.resolve(JobSpec::Current), Err(JobSpecError::NoSuchJob));
}

#[test]
fn test_resolve_previous() {
    let mut table = JobTable::default();
    let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
    let _id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
    assert_eq!(table.resolve(JobSpec::Previous), Ok(id1));
}

#[test]
fn test_resolve_previous_unset() {
    let mut table = JobTable::default();
    let _id = table.add_job(pid(1), vec![pid(1)], "a", false);
    // Only one job added — previous is unset
    assert_eq!(table.resolve(JobSpec::Previous), Err(JobSpecError::NoSuchJob));
}

#[test]
fn test_resolve_numeric_hit() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "x", false);
    assert_eq!(table.resolve(JobSpec::Numeric(id)), Ok(id));
}

#[test]
fn test_resolve_numeric_miss() {
    let table = JobTable::default();
    assert_eq!(table.resolve(JobSpec::Numeric(99)), Err(JobSpecError::NoSuchJob));
}

#[test]
fn test_resolve_prefix_single() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "vim README.md", false);
    table.add_job(pid(2), vec![pid(2)], "sleep 30", false);
    assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
}

#[test]
fn test_resolve_prefix_none() {
    let mut table = JobTable::default();
    table.add_job(pid(1), vec![pid(1)], "sleep 30", false);
    assert_eq!(
        table.resolve(JobSpec::Prefix("vim")),
        Err(JobSpecError::NoSuchJob)
    );
}

#[test]
fn test_resolve_prefix_ambiguous() {
    let mut table = JobTable::default();
    table.add_job(pid(1), vec![pid(1)], "sleep 10", false);
    table.add_job(pid(2), vec![pid(2)], "sleep 20", false);
    assert_eq!(
        table.resolve(JobSpec::Prefix("sleep")),
        Err(JobSpecError::Ambiguous)
    );
}

#[test]
fn test_resolve_substring_single() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "vim README.md", false);
    table.add_job(pid(2), vec![pid(2)], "sleep 30", false);
    assert_eq!(table.resolve(JobSpec::Substring("EADME")), Ok(id));
}

#[test]
fn test_resolve_substring_ambiguous() {
    let mut table = JobTable::default();
    table.add_job(pid(1), vec![pid(1)], "cat foo", false);
    table.add_job(pid(2), vec![pid(2)], "grep foo", false);
    assert_eq!(
        table.resolve(JobSpec::Substring("foo")),
        Err(JobSpecError::Ambiguous)
    );
}

#[test]
fn test_resolve_prefix_matches_done_job() {
    // bash-compatible: Prefix matches all statuses, including Done
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "vim foo", false);
    if let Some(job) = table.get_mut(id) {
        job.status = JobStatus::Done(0);
    }
    assert_eq!(table.resolve(JobSpec::Prefix("vim")), Ok(id));
}
```

- [ ] **Step 7: Run resolver tests — verify failure**

Run: `cargo test --lib env::jobs::tests::test_resolve`
Expected: compile errors (`JobTable::resolve` not found).

- [ ] **Step 8: Implement `JobTable::resolve`**

Add inside `impl JobTable`, immediately after the existing `resolve_job_spec` method (around line 222):

```rust
/// Resolve a parsed `JobSpec` to a `JobId`.
///
/// Matching is performed against `Job.command` (full command line),
/// case-sensitive, across all job statuses (Running, Stopped, Done,
/// Terminated) — bash-compatible.
///
/// Returns:
/// - `Ok(id)` if exactly one job matches
/// - `Err(NoSuchJob)` if no job matches
/// - `Err(Ambiguous)` if two or more jobs match (Prefix/Substring only)
pub fn resolve(&self, spec: JobSpec<'_>) -> Result<JobId, JobSpecError> {
    match spec {
        JobSpec::Current => self.current.ok_or(JobSpecError::NoSuchJob),
        JobSpec::Previous => self.previous.ok_or(JobSpecError::NoSuchJob),
        JobSpec::Numeric(n) => {
            if self.jobs.contains_key(&n) {
                Ok(n)
            } else {
                Err(JobSpecError::NoSuchJob)
            }
        }
        JobSpec::Prefix(s) => self.resolve_by(|cmd| cmd.starts_with(s)),
        JobSpec::Substring(s) => self.resolve_by(|cmd| cmd.contains(s)),
    }
}

/// Internal helper: scan all jobs and collapse match count to a Result.
fn resolve_by<F>(&self, mut pred: F) -> Result<JobId, JobSpecError>
where
    F: FnMut(&str) -> bool,
{
    let mut matched: Option<JobId> = None;
    for job in self.jobs.values() {
        if pred(&job.command) {
            if matched.is_some() {
                return Err(JobSpecError::Ambiguous);
            }
            matched = Some(job.id);
        }
    }
    matched.ok_or(JobSpecError::NoSuchJob)
}
```

- [ ] **Step 9: Run resolver tests — verify success**

Run: `cargo test --lib env::jobs::tests::test_resolve`
Expected: all 12 resolver tests pass.

- [ ] **Step 10: Run full jobs.rs test module and verify no regressions**

Run: `cargo test --lib env::jobs::tests`
Expected: all tests pass (both new and pre-existing). The existing `test_resolve_job_spec_*` tests still use the `Option<JobId>` API and should continue to pass unchanged.

- [ ] **Step 11: Commit**

```bash
git add src/env/jobs.rs
git commit -m "$(cat <<'EOF'
feat(jobs): add JobSpec enum, parser, and resolve method

Introduces JobSpec<'a> (Current/Previous/Numeric/Prefix/Substring),
JobSpecError (Malformed/NoSuchJob/Ambiguous), a pure zero-allocation
parse_job_spec function, and JobTable::resolve that handles prefix and
substring matching against Job.command with ambiguity detection. The
existing resolve_job_spec(&str) -> Option<JobId> API is retained
unchanged in this commit; callers will be migrated in a follow-up.

Matches bash semantics: case-sensitive, full-command matching, all job
statuses (Running/Stopped/Done/Terminated) eligible.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Migrate `resolve_job_spec` to `Result` API and update callers

**Files:**
- Modify: `src/env/jobs.rs` (the existing `resolve_job_spec` method + its 5 tests)
- Modify: `src/exec/mod.rs` (3 call sites around L382, L521, L581)

This task changes the public signature of `resolve_job_spec` from `Option<JobId>` to `Result<JobId, JobSpecError>`. Callers must be updated in the same commit to keep compilation green. The new caller code gains the `Ambiguous` error path that was unreachable before.

- [ ] **Step 1: Update existing `resolve_job_spec` tests in `src/env/jobs.rs`**

Replace the 5 existing `test_resolve_job_spec_*` tests (currently at `src/env/jobs.rs:575-610`) with the `Result`-returning versions:

```rust
#[test]
fn test_resolve_job_spec_numeric() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "x", false);
    assert_eq!(table.resolve_job_spec("%1"), Ok(id));
}

#[test]
fn test_resolve_job_spec_percent_percent() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "x", false);
    assert_eq!(table.resolve_job_spec("%%"), Ok(id));
}

#[test]
fn test_resolve_job_spec_plus() {
    let mut table = JobTable::default();
    let id = table.add_job(pid(1), vec![pid(1)], "x", false);
    assert_eq!(table.resolve_job_spec("%+"), Ok(id));
}

#[test]
fn test_resolve_job_spec_minus() {
    let mut table = JobTable::default();
    let id1 = table.add_job(pid(1), vec![pid(1)], "a", false);
    let _id2 = table.add_job(pid(2), vec![pid(2)], "b", false);
    assert_eq!(table.resolve_job_spec("%-"), Ok(id1));
}

#[test]
fn test_resolve_job_spec_invalid() {
    let table = JobTable::default();
    // "%99" — syntactically valid Numeric(99) but no such job
    assert_eq!(table.resolve_job_spec("%99"), Err(JobSpecError::NoSuchJob));
    // "foo" — doesn't start with '%'
    assert_eq!(table.resolve_job_spec("foo"), Err(JobSpecError::Malformed));
    // "%abc" — Prefix("abc") against empty table → NoSuchJob (previously Malformed)
    assert_eq!(table.resolve_job_spec("%abc"), Err(JobSpecError::NoSuchJob));
}

#[test]
fn test_resolve_job_spec_ambiguous() {
    let mut table = JobTable::default();
    table.add_job(pid(1), vec![pid(1)], "sleep 10", false);
    table.add_job(pid(2), vec![pid(2)], "sleep 20", false);
    assert_eq!(
        table.resolve_job_spec("%sleep"),
        Err(JobSpecError::Ambiguous)
    );
}
```

Note: the third assertion in `test_resolve_job_spec_invalid` changes meaning versus the old test — `%abc` used to return `None` because it wasn't a recognized form; now it parses as `Prefix("abc")` and resolves to `NoSuchJob` against the empty table. The `Err` side is still the right assertion, just with a different variant.

- [ ] **Step 2: Reshape `resolve_job_spec` method signature**

Replace the existing `resolve_job_spec` method in `src/env/jobs.rs` (currently at lines 201-221) with:

```rust
/// Resolve a job specification string to a JobId.
///
/// Supported forms (see `parse_job_spec` for syntax):
/// - `%%` / `%+` — current job
/// - `%-` — previous job
/// - `%n` — job by numeric id
/// - `%string` — command begins with string
/// - `%?string` — command contains string
///
/// Returns `Err(Ambiguous)` when a Prefix/Substring spec matches 2+ jobs.
pub fn resolve_job_spec(&self, spec: &str) -> Result<JobId, JobSpecError> {
    self.resolve(parse_job_spec(spec)?)
}
```

- [ ] **Step 3: Update `wait` caller in `src/exec/mod.rs`**

Note: do not try to run `cargo test` between this step and Step 2 — `src/exec/mod.rs` still uses the old `Option` API after Step 2, so the whole crate won't compile until all three callers are migrated. The next three steps do that migration atomically.

Replace `src/exec/mod.rs:380-397` (the `else` branch inside `builtin_wait`). Current code:

```rust
let mut pids = Vec::new();
for arg in args {
    if let Some(job_id) = self.env.process.jobs.resolve_job_spec(arg) {
        if let Some(job) = self.env.process.jobs.get(job_id) {
            pids.push(job.pgid);
        } else {
            return Err(ShellError::runtime(RuntimeErrorKind::CommandNotFound, format!("wait: {}: no such job", arg)));
        }
    } else {
        match arg.parse::<i32>() {
            Ok(n) => pids.push(Pid::from_raw(n)),
            Err(_) => {
                return Err(ShellError::runtime(RuntimeErrorKind::InvalidArgument, format!("wait: {}: not a pid or valid job spec", arg)));
            }
        }
    }
}
pids
```

Replace with:

```rust
let mut pids = Vec::new();
for arg in args {
    if arg.starts_with('%') {
        match self.env.process.jobs.resolve_job_spec(arg) {
            Ok(job_id) => {
                if let Some(job) = self.env.process.jobs.get(job_id) {
                    pids.push(job.pgid);
                } else {
                    return Err(ShellError::runtime(RuntimeErrorKind::CommandNotFound, format!("wait: {}: no such job", arg)));
                }
            }
            Err(crate::env::jobs::JobSpecError::Ambiguous) => {
                return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("wait: {}: ambiguous job spec", arg)));
            }
            Err(_) => {
                return Err(ShellError::runtime(RuntimeErrorKind::CommandNotFound, format!("wait: {}: no such job", arg)));
            }
        }
    } else {
        match arg.parse::<i32>() {
            Ok(n) => pids.push(Pid::from_raw(n)),
            Err(_) => {
                return Err(ShellError::runtime(RuntimeErrorKind::InvalidArgument, format!("wait: {}: not a pid or valid job spec", arg)));
            }
        }
    }
}
pids
```

The PID fallback is now gated on `!arg.starts_with('%')` so a `%foo` argument always goes through the job-spec path and reports `"no such job"` / `"ambiguous job spec"` rather than falling through to the PID parser.

- [ ] **Step 4: Update `fg` caller in `src/exec/mod.rs`**

Replace `src/exec/mod.rs:520-527` (the `else` branch inside `builtin_fg`). Current code:

```rust
match self.env.process.jobs.resolve_job_spec(&args[0]) {
    Some(id) => id,
    None => {
        return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("fg: {}: no such job", args[0])));
    }
}
```

Replace with:

```rust
match self.env.process.jobs.resolve_job_spec(&args[0]) {
    Ok(id) => id,
    Err(crate::env::jobs::JobSpecError::Ambiguous) => {
        return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("fg: {}: ambiguous job spec", args[0])));
    }
    Err(_) => {
        return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("fg: {}: no such job", args[0])));
    }
}
```

- [ ] **Step 5: Update `bg` caller in `src/exec/mod.rs`**

Replace `src/exec/mod.rs:580-587` (the `else` branch inside `builtin_bg`). Current code:

```rust
match self.env.process.jobs.resolve_job_spec(&args[0]) {
    Some(id) => id,
    None => {
        return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("bg: {}: no such job", args[0])));
    }
}
```

Replace with:

```rust
match self.env.process.jobs.resolve_job_spec(&args[0]) {
    Ok(id) => id,
    Err(crate::env::jobs::JobSpecError::Ambiguous) => {
        return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("bg: {}: ambiguous job spec", args[0])));
    }
    Err(_) => {
        return Err(ShellError::runtime(RuntimeErrorKind::JobControlError, format!("bg: {}: no such job", args[0])));
    }
}
```

- [ ] **Step 6: Build the whole library**

Run: `cargo build`
Expected: clean build, no errors, no warnings related to this change.

- [ ] **Step 7: Run the full unit + integration test suite**

Run: `cargo test`
Expected: all tests pass. Watch especially for any existing integration test that may have been exercising the Option signature.

- [ ] **Step 8: Commit**

```bash
git add src/env/jobs.rs src/exec/mod.rs
git commit -m "$(cat <<'EOF'
feat(jobs): migrate resolve_job_spec to Result API and add %string/%?string

JobTable::resolve_job_spec now returns Result<JobId, JobSpecError>
instead of Option<JobId>. Callers in builtin_wait, builtin_fg, and
builtin_bg distinguish Ambiguous (emit "ambiguous job spec") from all
other errors (emit "no such job"), matching bash.

Because %string and %?string are now parsed, wait gates its PID
fallback on `!arg.starts_with('%')` so `wait %bogus` reports
"no such job" instead of "not a pid or valid job spec".

Closes the POSIX §3.204 gap tracked in TODO.md under
"Job Control: Known Limitations".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add E2E tests for job spec string matching

**Files:**
- Create: `e2e/builtin/job_spec_prefix.sh` (644)
- Create: `e2e/builtin/job_spec_substring.sh` (644)
- Create: `e2e/builtin/job_spec_ambiguous.sh` (644)
- Create: `e2e/builtin/job_spec_nomatch.sh` (644)

All four scripts use `wait` rather than `fg`/`bg` because `wait` works without `set -m` (see existing `e2e/builtin/fg_no_monitor.sh` / `bg_no_monitor.sh`), and because `builtin_wait` already routes job-spec arguments through `resolve_job_spec`.

- [ ] **Step 1: Create `e2e/builtin/job_spec_prefix.sh`**

Write the file (mode 644 — do not use `chmod +x`):

```sh
#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %string prefix match resolves a unique background job
# EXPECT_EXIT: 0
sleep 0.1 &
wait %sleep
```

- [ ] **Step 2: Create `e2e/builtin/job_spec_substring.sh`**

```sh
#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %?string substring match resolves a unique background job
# EXPECT_EXIT: 0
sleep 0.1 &
wait %?leep
```

- [ ] **Step 3: Create `e2e/builtin/job_spec_ambiguous.sh`**

The trailing `wait` with no args drains the remaining background jobs so the script doesn't leak processes; `$status` preserves the exit code of the tested `wait %sleep` because `wait` with no args overwrites `$?` with 0.

```sh
#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %string matching two jobs reports ambiguous job spec
# EXPECT_STDERR: ambiguous job spec
# EXPECT_EXIT: 1
sleep 0.1 &
sleep 0.2 &
wait %sleep
status=$?
wait
exit $status
```

- [ ] **Step 4: Create `e2e/builtin/job_spec_nomatch.sh`**

```sh
#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %string with no matching job reports no such job
# EXPECT_STDERR: no such job
# EXPECT_EXIT: 1
sleep 0.1 &
wait %bogus
status=$?
wait
exit $status
```

- [ ] **Step 5: Verify file permissions**

Run: `ls -l e2e/builtin/job_spec_*.sh`
Expected: all four files report `-rw-r--r--` (644). If any show `-rwxr-xr-x`, run `chmod 644 e2e/builtin/job_spec_*.sh`.

- [ ] **Step 6: Run the new E2E tests**

Run: `cargo build && ./e2e/run_tests.sh --filter=job_spec`
Expected: all 4 tests pass. The runner prints a summary line like `4/4 passed`.

- [ ] **Step 7: Run the full E2E suite to check for regressions**

Run: `./e2e/run_tests.sh`
Expected: no new failures vs. baseline (pre-existing failures, if any, remain unchanged).

- [ ] **Step 8: Commit**

```bash
git add e2e/builtin/job_spec_prefix.sh e2e/builtin/job_spec_substring.sh e2e/builtin/job_spec_ambiguous.sh e2e/builtin/job_spec_nomatch.sh
git commit -m "$(cat <<'EOF'
test(e2e): add %string/%?string job specifier tests

Four new scripts under e2e/builtin/ covering the success paths
(prefix/substring resolving a unique job) and the failure paths
(ambiguous match, no match).

All scripts use `wait` (no `set -m` requirement) and preserve the
ambiguous/no-such-job exit status via a temp variable so the script
exits with the tested status even though a trailing `wait` drains the
remaining background jobs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Remove TODO bullet and final verification

**Files:**
- Modify: `TODO.md` (delete one line in the Job Control section)

- [ ] **Step 1: Remove the `%string` / `%?string` bullet from `TODO.md`**

Delete exactly this line from `TODO.md` (currently line 5):

```
- [ ] `%string` / `%?string` job specifiers — prefix/substring matching not implemented
```

Use the Edit tool with `old_string` = the exact line above and `new_string` = empty, or delete the bullet and collapse adjacent blank lines as appropriate so the Job Control section reads naturally.

- [ ] **Step 2: Run the full unit + integration test suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 3: Run the full E2E suite**

Run: `cargo build && ./e2e/run_tests.sh`
Expected: baseline pass count; no new failures.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "$(cat <<'EOF'
docs(TODO): remove completed %string/%?string job specifier bullet

Prefix (%string) and substring (%?string) job specifiers are now
implemented with bash-compatible ambiguity detection.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Summary

4 tasks, ~30 steps total, producing 4 commits:

1. **Task 1** — Add `JobSpec` / `JobSpecError` types, `parse_job_spec` function, and `JobTable::resolve` method (pure addition, 26 new tests)
2. **Task 2** — Migrate `resolve_job_spec` return type from `Option` to `Result`; update 3 callers in `src/exec/mod.rs` with bash-compatible error messages
3. **Task 3** — Add 4 E2E scripts under `e2e/builtin/` covering success and failure paths
4. **Task 4** — Remove the implemented bullet from `TODO.md` and run final verification
