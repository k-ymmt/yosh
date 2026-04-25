# Terminal-State Save/Restore: Follow-up Cleanup Design

**Date:** 2026-04-26

**Status:** Approved for implementation

## Background

The terminal-state save/restore feature for job control landed across commits
`9deab18..4e04dfc` (2026-04-24 ~ 2026-04-26). The feature follows the GNU libc
manual "Implementing a Job Control Shell" pattern: capture TTY termios on
`WaitStatus::Stopped`, replay it on `fg`, and restore the shell's snapshot
after every foreground wait.

The final code-review pass on that work surfaced four follow-ups, recorded in
`TODO.md` (lines 10–13 at the time of writing):

- L10 — `Job.saved_tmodes` is `pub`; the "written only by `wait_for_foreground_job`
  on `WaitStatus::Stopped`" invariant is not enforced by the type.
- L11 — `wait_for_foreground_job` Stopped-arm `captured.is_some()` guard
  preserves a stale termios when `capture_tty_termios` returns `None` (the
  `exec 0</dev/null` mid-session case).
- L12 — `wait_for_foreground_job` docstring does not mention the
  `job.saved_tmodes` side-effect.
- L13 — `Repl::new` `is_interactive && monitor` guard is currently redundant
  but kept for symmetry; the intent is undocumented.

This spec covers all four. **L9** (`shell_tmodes` is a startup snapshot that
`stty` does not refresh) is explicitly out of scope: it matches the glibc
manual semantics and is parked until a user reports the surface.

## Goals

1. Fix the `captured.is_some()` guard so a stale `saved_tmodes` cannot survive
   into a later `fg` after stdin has been redirected away from the TTY.
2. Tighten `Job.saved_tmodes` to `private` + accessor pair, matching the
   pattern already applied to `JobTable.shell_tmodes`.
3. Make `wait_for_foreground_job`'s Stopped-arm side-effect discoverable by
   `grep saved_tmodes` and by reading the docstring.
4. Annotate the redundant `Repl::new` guard so it is not silently
   "simplified" away.

## Non-Goals

- Re-architecting termios capture (e.g., reacting to runtime `stty`).
- Changing the `fg` fallback behavior for jobs that were never stopped.
- Cross-platform termios serialization (still single-process).

## Design

### 1. Bug Fix: Drop `captured.is_some()` Guard

**File:** `src/exec/mod.rs` (Stopped arm of `wait_for_foreground_job`)

Replace

```rust
if captured.is_some() {
    job.saved_tmodes = captured;
}
```

with an unconditional assignment via the new setter (see §2):

```rust
job.set_saved_tmodes(captured);
```

**Why:** `captured = None` happens when interactive+monitor is on but
`capture_tty_termios()` returns `Ok(None)` because stdin is no longer a TTY
(typical mid-session trigger: `exec 0</dev/null`). In that state, retaining a
prior snapshot tells a future `fg` to apply termios for a TTY that the shell
no longer drives — the apply will either silently no-op or scribble onto an
unrelated fd that happens to be at fd 0. Unconditional overwrite matches the
glibc manual semantics ("save what's there now, or nothing").

The current behavior is asymptomatic in the existing test matrix because no
test transitions through `exec 0</dev/null` between two stops, but the latent
bug is real and the simpler form is also easier to reason about.

### 2. Private `Job.saved_tmodes` + Accessor Pair

**File:** `src/env/jobs.rs`

Mirror the `JobTable.shell_tmodes` pattern:

```rust
pub struct Job {
    // ...
    saved_tmodes: Option<nix::sys::termios::Termios>,  // was: pub
}

impl Job {
    /// Termios snapshot captured the last time this job stopped (SIGTSTP/
    /// SIGSTOP), or `None` if it has never stopped or capture was unavailable.
    pub fn saved_tmodes(&self) -> Option<&nix::sys::termios::Termios> {
        self.saved_tmodes.as_ref()
    }

    /// Replace the saved termios snapshot. Intended only for the
    /// `WaitStatus::Stopped` branch of foreground-wait — passing `None`
    /// is valid and clears any previously stored value.
    pub fn set_saved_tmodes(&mut self, t: Option<nix::sys::termios::Termios>) {
        self.saved_tmodes = t;
    }
}
```

Field initialization in `add_job` stays as `saved_tmodes: None` (struct
literal in the same module — visibility is fine).

**Call-site updates:**
- `src/exec/mod.rs:696`: `.and_then(|j| j.saved_tmodes.clone())`
  → `.and_then(|j| j.saved_tmodes().cloned())`
- `src/exec/mod.rs:877`: `job.saved_tmodes = captured`
  → `job.set_saved_tmodes(captured)` (combined with §1)
- `src/env/jobs.rs:497` (existing test): `job.saved_tmodes.is_none()`
  → `job.saved_tmodes().is_none()`

### 3. Docstring Update for `wait_for_foreground_job`

**File:** `src/exec/mod.rs:807-810`

Extend the existing docstring to mention the Stopped-arm side-effect so a
future maintainer running `grep saved_tmodes` lands on it:

```rust
/// Wait for a foreground job to complete or stop.
///
/// Returns a `ForegroundWaitResult` containing the last exit status,
/// per-process statuses (for pipefail), and whether the job was stopped.
///
/// Side effect: on `WaitStatus::Stopped`, captures the current TTY termios
/// (or `None` when stdin is not a TTY / non-interactive / non-monitor) into
/// `job.saved_tmodes` so a later `fg` can replay it. The capture is always
/// written — including `None` overwrites — to avoid keeping a stale snapshot
/// across `exec 0</dev/null` style redirections.
fn wait_for_foreground_job(...) { ... }
```

### 4. `Repl::new` Guard Comment

**File:** `src/interactive/mod.rs:54`

The guard `if executor.env.mode.is_interactive && executor.env.mode.options.monitor`
is currently redundant — both flags are set to `true` two lines above
(`src/interactive/mod.rs:44-45`). The guard is kept for symmetry with the
runtime-conditional `restore_shell_termios_if_interactive` call site, where
the same check IS load-bearing.

Add a one-line comment to record this so the guard is not "simplified" away:

```rust
// Guard mirrors `restore_shell_termios_if_interactive`; the flags above are
// unconditionally true here, so the check is documentation-only at this
// site but load-bearing at the symmetric one in `wait_for_foreground_job`.
if executor.env.mode.is_interactive && executor.env.mode.options.monitor {
    if let Ok(Some(t)) = crate::exec::terminal_state::capture_tty_termios() {
        executor.env.process.jobs.set_shell_tmodes(t);
    }
}
```

## Tests

- **Existing** `test_job_saved_tmodes_defaults_none` — rewrite the assertion
  to use the `saved_tmodes()` accessor. Same intent, new shape.
- **New** `test_job_set_saved_tmodes_overwrites_with_none` —
  - call `set_saved_tmodes(Some(t))`, assert `saved_tmodes().is_some()`,
  - call `set_saved_tmodes(None)`, assert `saved_tmodes().is_none()`.
  This pins the §1 fix at the unit level: it asserts that a `None` write
  must clear prior state, which is exactly the bug the `captured.is_some()`
  guard caused.
- **PTY tests** (`tests/pty_interactive.rs`) — no source changes; they pass
  by API compatibility. The Ctrl-Z → bg → fg termios cycle and the post-fg
  `stty` round-trip already cover the unchanged restore paths.

No new PTY test for the §1 bug surface itself: the trigger requires
`exec 0</dev/null` mid-session followed by a Ctrl-Z then `fg`, and the
existing PTY harness has no clean way to redirect the shell's stdin away
from the master fd without confusing `expectrl`. The unit test in §Tests
covers the type-level invariant the bug violated.

## TODO.md Updates

Delete entries L10, L11, L12, L13 from `TODO.md` (CLAUDE.md convention:
remove completed items, do not mark `[x]`).

## Risks / Open Questions

- **API churn.** `Job.saved_tmodes` is a public field today; downstream
  code outside the workspace would break. There is no such code: the
  type is shell-internal and `Job` is not re-exported.
- **Compile-time visibility.** Initializing the private field via struct
  literal in `add_job` works because the literal is in the same module.
  No setter is needed for the initial `None`.
- **Drop semantics.** `Termios` is `Clone` and stack-sized; `set_saved_tmodes`
  taking `Option<Termios>` by value is fine (no perf concern).

## Out of Scope

- L9: `JobTable.shell_tmodes` does not refresh on runtime `stty`. Matches
  glibc manual; revisit if user reports surface.
- Other entries above L9 in the L8 block (Task 7 fg job-termios PTY
  assertion). Those involve the `expectrl` PTY harness limitations on
  macOS/BSD `read()` SIGCONT semantics; an independent investigation.
