# Job Spec String Matching (`%string` / `%?string`)

**Date:** 2026-04-17
**Status:** Draft — pending user review
**POSIX Reference:** §3.204 Job Control Job ID, §2.9.1.1

## Problem

yosh currently supports only the numeric and anchor forms of POSIX job specifiers: `%n`, `%%`, `%+`, `%-`. The prefix form `%string` and the substring form `%?string` are unimplemented, leaving a documented gap under **Job Control: Known Limitations** in `TODO.md`. POSIX compliance is the project's primary goal, so closing this gap matters.

Current behavior (`src/env/jobs.rs:208 resolve_job_spec`) returns `Option<JobId>` and silently returns `None` for any unrecognized input, which gives callers no way to distinguish "no such job" from "ambiguous match" even after the feature is added.

## Goals

- Implement POSIX `%string` (prefix match on command) and `%?string` (substring match on command)
- Emit bash-compatible error messages: `"no such job"` vs. `"ambiguous job spec"`
- Keep the parser pure and independently testable
- Preserve existing `%n` / `%%` / `%+` / `%-` semantics unchanged

## Non-Goals

- Case-insensitive matching (zsh `MATCH_NO_CASE`) — not in POSIX
- Backslash-escape support (`%\?foo`) — not in POSIX
- kill builtin integration — handled separately if/when kill supports job specs
- Per-process status tracking / `$PIPESTATUS` — tracked as a separate TODO item

## Design

### Semantics

| Spec          | Meaning                                          |
|---------------|--------------------------------------------------|
| `%%`, `%+`    | current job                                      |
| `%-`          | previous job                                     |
| `%n`          | job with numeric id `n`                          |
| `%string`     | job whose `command` field **begins with** string |
| `%?string`    | job whose `command` field **contains** string    |

Matching is performed against `Job.command` as stored (the full command line), case-sensitive, against all jobs regardless of status (`Running`, `Stopped`, `Done`, `Terminated`). This matches bash behavior.

If a `Prefix` or `Substring` spec matches zero jobs, it is a `NoSuchJob` error. If it matches two or more jobs, it is an `Ambiguous` error. Exactly one match resolves successfully.

### Types

Added to `src/env/jobs.rs`:

```rust
pub enum JobSpec<'a> {
    Current,             // %%  or  %+
    Previous,            // %-
    Numeric(JobId),      // %n
    Prefix(&'a str),     // %string
    Substring(&'a str),  // %?string
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSpecError {
    Malformed,   // "", "%", "%?", "foo", bad number, etc.
    NoSuchJob,   // parse OK but nothing matches
    Ambiguous,   // 2+ matches for Prefix/Substring
}
```

`JobSpec` borrows from the input string to avoid heap allocation in the hot path.

### Parser

```rust
pub fn parse_job_spec(s: &str) -> Result<JobSpec<'_>, JobSpecError>;
```

Disambiguation order (earliest match wins):

1. `"%%"` or `"%+"` → `Current`
2. `"%-"` → `Previous`
3. `"%<digits>"` with non-empty digit run → `Numeric(n)` (reject on parse overflow → `Malformed`)
4. `"%?<rest>"` with non-empty `rest` → `Substring(rest)`
5. `"%<rest>"` with non-empty `rest` → `Prefix(rest)`
6. Everything else (`""`, `"%"`, `"%?"`, input not starting with `%`, etc.) → `Malformed`

Note edge case: `%%foo` falls to case 5 → `Prefix("%foo")`, which is harmless (no job's command starts with `%foo`).

### Resolver

```rust
impl JobTable {
    pub fn resolve(&self, spec: JobSpec) -> Result<JobId, JobSpecError>;

    pub fn resolve_job_spec(&self, s: &str) -> Result<JobId, JobSpecError> {
        self.resolve(parse_job_spec(s)?)
    }
}
```

`resolve` dispatches on the variant:

- `Current` → `self.current.ok_or(NoSuchJob)`
- `Previous` → `self.previous.ok_or(NoSuchJob)`
- `Numeric(n)` → `if self.jobs.contains_key(&n) { Ok(n) } else { Err(NoSuchJob) }`
- `Prefix(s)` → iterate `self.jobs.values()`, collect `job.id` where `job.command.starts_with(s)`; 0 → `NoSuchJob`, 1 → `Ok`, ≥2 → `Ambiguous`
- `Substring(s)` → same as `Prefix` but with `contains`

The existing `resolve_job_spec` public method is reshaped from `Option<JobId>` to `Result<JobId, JobSpecError>`. There is no `Option`-returning variant retained.

### Caller Updates (`src/exec/mod.rs`)

Three call sites: L382 (`wait`), L521 (`fg`), L581 (`bg`). Each becomes:

```rust
match self.env.process.jobs.resolve_job_spec(arg) {
    Ok(id) => { /* existing success path */ }
    Err(JobSpecError::Ambiguous) => {
        return Err(ShellError::runtime(
            RuntimeErrorKind::JobControlError,
            format!("{}: {}: ambiguous job spec", name, arg),
        ));
    }
    Err(_) => {
        return Err(ShellError::runtime(
            RuntimeErrorKind::JobControlError,
            format!("{}: {}: no such job", name, arg),
        ));
    }
}
```

`Malformed` and `NoSuchJob` collapse to the same `"no such job"` message (bash parity). The `wait` builtin uses `RuntimeErrorKind::CommandNotFound` today for `"no such job"` — we keep that to avoid unrelated churn and only add the `Ambiguous` branch.

## Testing

### Unit Tests (`src/env/jobs.rs` tests module)

**Parser (`parse_job_spec`):**
- All accepted forms: `%%`, `%+`, `%-`, `%1`, `%99`, `%foo`, `%?bar`, `%-foo` (prefix `-foo`), `%%foo` (prefix `%foo`)
- Rejections: `""`, `"%"`, `"%?"`, `"foo"`, `"%?"`, overflow like `%99999999999999999999`

**Resolver (`JobTable::resolve`):**
- `Prefix` with single match → `Ok`
- `Prefix` with multiple matches → `Ambiguous`
- `Prefix` with zero matches → `NoSuchJob`
- `Substring` equivalent cases
- `Prefix` matches a `Done` job (all-status coverage)
- `Current` / `Previous` when unset → `NoSuchJob`

**Integration (`resolve_job_spec`):**
- Existing `test_resolve_job_spec_*` tests migrated to `Result` API
- Add ambiguity test: two jobs with overlapping prefix

### E2E Tests (`e2e/builtin/`)

Four new scripts using the existing conventions (644 perms, metadata headers):

- `job_spec_prefix.sh` — start one `sleep` in background, then `fg %sleep` or `wait %sleep` should succeed
- `job_spec_substring.sh` — same but with `%?leep`
- `job_spec_ambiguous.sh` — start two `sleep` jobs, `wait %sle` should emit `"ambiguous job spec"` on stderr and exit nonzero
- `job_spec_nomatch.sh` — `wait %bogus` should emit `"no such job"` and exit nonzero

`wait` is chosen over `fg`/`bg`/`kill` because (a) it is a special builtin with job-spec support already wired at L382, (b) existing `e2e/builtin/jobs_background.sh` already uses the `sleep … & ; wait` pattern, and (c) `kill` builtin job-spec support is out of scope.

## Risks / Open Questions

- **Edge case `%0`:** POSIX doesn't forbid it but no job id 0 exists in yosh (next_id starts at 1). `Numeric(0)` will resolve to `NoSuchJob`, which is correct.
- **Command string content:** if a command contains leading whitespace or a pipe (`foo | bar`), `%foo` still matches because the stored `command` starts with `foo`. This is bash-consistent.
- **API break of `resolve_job_spec`:** the return type changes from `Option<JobId>` to `Result<JobId, JobSpecError>`. All three known callers are in-tree and will be updated in the same change. External crates (plugin SDK etc.) do not use this API.

## Files Touched

- `src/env/jobs.rs` — add `JobSpec`, `JobSpecError`, `parse_job_spec`, `JobTable::resolve`; reshape `resolve_job_spec`; update unit tests
- `src/exec/mod.rs` — update three `resolve_job_spec` call sites (L382, L521, L581)
- `e2e/builtin/job_spec_prefix.sh` (new, 644)
- `e2e/builtin/job_spec_substring.sh` (new, 644)
- `e2e/builtin/job_spec_ambiguous.sh` (new, 644)
- `e2e/builtin/job_spec_nomatch.sh` (new, 644)
- `TODO.md` — remove the now-implemented bullet under **Job Control: Known Limitations**
