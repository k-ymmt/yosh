# POSIX Chapter 2 Conformance Gaps — Sub-project 5: §2.11 ignored-on-entry + §2.10.2 Rule 5

**Date**: 2026-04-20
**Sub-project**: 5 of 5 (POSIX Chapter 2 conformance gap remediation — final)
**Scope items from TODO.md**:

- §2.11 ignored-on-entry signal inheritance — no in-harness test yet (nested `sh -c` escapes yosh); revisit after a yosh-aware subshell helper lands
- §2.10.2 Rule 5 — yosh accepts reserved words as `for` NAME (`e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh` XFAIL). POSIX requires NAME to be a valid name, not a reserved word.

## Context

Sub-projects 1–4 have progressively closed the eight POSIX Chapter 2
conformance gap items listed in `TODO.md`. Two items remain, and both
are independent of each other:

1. **§2.11 ignored-on-entry**: POSIX §2.11 says *"Signals that were
   ignored on entry to a non-interactive shell cannot be trapped or
   reset, although no error need be reported when attempting to do
   so."* yosh currently violates this: `init_signal_handling()` in
   `src/signal.rs` installs its own `sigaction` handlers for all
   `HANDLED_SIGNALS`, unconditionally overwriting any inherited
   `SIG_IGN` disposition. The `trap` builtin in `src/builtin/special.rs`
   has no notion of "this signal was ignored on entry", so it happily
   accepts `trap 'cmd' SIGINT` even when the parent process ignored
   `SIGINT`. This allows a yosh script to override a parent's explicit
   ignore decision — a POSIX conformance break.
2. **§2.10.2 Rule 5**: POSIX §2.10.2 Rule 5 says the NAME in `for
   NAME in WORDS; do ...; done` must be a valid name (not a reserved
   word). yosh's `parse_for_clause` at `src/parser/mod.rs:529`
   accepts any identifier, including reserved words like `if` /
   `time`. `e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh`
   is an XFAIL test pinning the gap.

The prior obstacle for §2.11 testing — "nested `sh -c` escapes yosh"
— refers to the fact that the natural E2E pattern `sh -c 'trap "" TERM;
exec yosh -c "..."'` invokes the *system* `sh`, not yosh, to set up
the ignored-on-entry state. The test observes yosh's post-entry
behaviour correctly, but the setup bypasses yosh's own test harness.
This sub-project resolves that by moving the ignored-on-entry test
coverage into Rust integration tests (`tests/ignored_on_entry.rs`)
that `fork` + `sigaction(SIG_IGN)` + `exec` the yosh binary directly,
all within `cargo test`.

Scope decision for §2.11: this sub-project treats **both
non-interactive and interactive** shells as preserving ignored-on-entry
(matching bash). POSIX permits interactive shells to discard such
signals, but unifying the behaviour simplifies the implementation and
removes a surprising mode-dependent divergence for users.

## Goals

1. Capture the set of signals inherited with `SIG_IGN` disposition at
   shell startup, before any yosh handler is installed, via
   `sigaction(_, None, &mut old)` queries in a new
   `capture_ignored_on_entry` helper.
2. Skip yosh handler registration for ignored-on-entry signals so
   they remain `SIG_IGN` throughout the shell's lifetime.
3. `TrapStore::set_trap` / `remove_trap` silently succeed (no state
   change, exit 0) when targeting an ignored-on-entry signal.
4. `TrapStore::display_all` lists ignored-on-entry signals as
   `trap -- '' SIG<NAME>` so `trap` / `trap -p` output reflects the
   effective disposition (matches bash).
5. `reset_child_signals` unions ignored-on-entry with explicit
   `TrapAction::Ignore` so subshells, external commands, and command
   substitutions inherit the ignore disposition.
6. `parse_for_clause` rejects reserved words as NAME with a syntax
   error (exit 2, `yosh:` stderr prefix).
7. Remove the two remaining POSIX Chapter 2 gap items from `TODO.md`
   and delete the now-empty section heading; the Chapter 2 gap
   remediation series is complete.

## Non-goals

- §2.10.2 Rule 8 (NAME in function definition) — same bug class but
  not listed in `TODO.md`; tracked as a separate future item.
- Normative-granularity coverage for §2.11 or §2.10.2 (tracked
  separately by the "POSIX Chapter 2 normative-clause saturation"
  future item).
- Re-architecting `TrapStore` or the signal subsystem beyond what
  this change requires.
- Non-POSIX signals (SIGWINCH, SIGINFO, etc.).
- `KILL` / `STOP` special handling — OS forbids ignoring these
  signals, so they never appear in the ignored-on-entry set.
- Bash-specific extensions (`trap -l`, debugging traps, ERR/DEBUG
  pseudo-signals).

## Architecture

Two independent changes sharing a single sub-project umbrella:

### §2.11 ignored-on-entry — three-layer change

| Layer | File | Change |
|---|---|---|
| Signal subsystem | `src/signal.rs` | Add `IGNORED_ON_ENTRY: OnceLock<HashSet<i32>>`, `capture_ignored_on_entry()`, `is_ignored_on_entry(sig)`, `ignored_on_entry_set()`. `init_signal_handling()` captures entry set first, then skips handler registration for those signals. `reset_child_signals(ignored)` internally unions with entry set. |
| Trap store | `src/env/traps.rs` | `set_trap` / `remove_trap` delegate to new `set_trap_with` / `remove_trap_with` that take a `&dyn Fn(i32) -> bool` ignored-predicate (dependency injection for unit testing). Public API silently no-ops when the predicate returns true for a signal. `display_all` unions entry set into its key iteration so ignored-on-entry signals appear as `trap -- '' SIG<NAME>`. |
| Builtin trap | `src/builtin/special.rs` | No direct change — it already delegates through `TrapStore`. |

### §2.10.2 Rule 5 — single-layer parser change

| Layer | File | Change |
|---|---|---|
| Parser | `src/parser/mod.rs` | `parse_for_clause` adds `is_reserved_name(&name)` check immediately after NAME extraction; returns a syntax error on reserved-word match. |

### New signal API

```rust
// src/signal.rs

use std::collections::HashSet;

static IGNORED_ON_ENTRY: OnceLock<HashSet<i32>> = OnceLock::new();

fn capture_ignored_on_entry() -> HashSet<i32> {
    let mut set = HashSet::new();
    for &(num, _) in SIGNAL_TABLE {
        if num == libc::SIGKILL || num == libc::SIGSTOP {
            continue;
        }
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sigaction(num, std::ptr::null(), &mut old) };
        if rc != 0 {
            continue;
        }
        if old.sa_sigaction == libc::SIG_IGN {
            set.insert(num);
        }
    }
    set
}

pub fn is_ignored_on_entry(sig: i32) -> bool {
    IGNORED_ON_ENTRY
        .get()
        .map_or(false, |set| set.contains(&sig))
}

pub fn ignored_on_entry_set() -> &'static HashSet<i32> {
    IGNORED_ON_ENTRY
        .get()
        .expect("init_signal_handling() must be called first")
}
```

### `init_signal_handling` integration

```rust
pub fn init_signal_handling() {
    SELF_PIPE.get_or_init(|| {
        // NEW: capture BEFORE installing any yosh handler.
        IGNORED_ON_ENTRY.get_or_init(capture_ignored_on_entry);

        // ... existing pipe creation ...

        for &(num, _) in HANDLED_SIGNALS {
            // NEW: preserve inherited SIG_IGN.
            if IGNORED_ON_ENTRY.get().unwrap().contains(&num) {
                continue;
            }
            // ... existing sigaction registration ...
        }

        (read_fd, write_fd)
    });
}
```

Ordering invariant: `init_signal_handling` is called first at every
shell entry point (`run_string`, `run_file`, `Repl::new`).
`init_job_control_signals` (interactive mode only) runs afterwards
and explicitly sets `SIGTSTP/TTIN/TTOU` to `SIG_IGN` for job
control — this is yosh's own decision and is *not* treated as
ignored-on-entry even though the net disposition is identical.

### `reset_child_signals` union

```rust
pub fn reset_child_signals(ignored: &[i32]) {
    let entry_set = IGNORED_ON_ENTRY.get();
    for &(num, _) in HANDLED_SIGNALS {
        let keep_ignored = ignored.contains(&num)
            || entry_set.map_or(false, |s| s.contains(&num));
        if keep_ignored {
            ignore_signal(num);
        } else {
            default_signal(num);
        }
    }
    // ... existing self-pipe close ...
}
```

Callers (`exec_subshell`, `exec_external`, `exec_background`,
`setup_foreground_child_signals`, `setup_background_child_signals`)
need no change — the union is absorbed inside `reset_child_signals`.

### `TrapStore` dependency-injection API

```rust
// src/env/traps.rs

impl TrapStore {
    pub fn set_trap(&mut self, condition: &str, action: TrapAction) -> Result<(), String> {
        self.set_trap_with(condition, action, &|sig| {
            crate::signal::is_ignored_on_entry(sig)
        })
    }

    pub(crate) fn set_trap_with(
        &mut self,
        condition: &str,
        action: TrapAction,
        is_ignored: &dyn Fn(i32) -> bool,
    ) -> Result<(), String> {
        let num = Self::signal_name_to_number(condition)
            .ok_or_else(|| format!("invalid signal name: {}", condition))?;
        if num == 0 {
            self.exit_trap = Some(action);
            return Ok(());
        }
        if is_ignored(num) {
            return Ok(()); // POSIX §2.11: silent no-op
        }
        self.signal_traps.insert(num, action);
        Ok(())
    }

    pub fn remove_trap(&mut self, condition: &str) {
        self.remove_trap_with(condition, &|sig| {
            crate::signal::is_ignored_on_entry(sig)
        })
    }

    pub(crate) fn remove_trap_with(
        &mut self,
        condition: &str,
        is_ignored: &dyn Fn(i32) -> bool,
    ) {
        let Some(num) = Self::signal_name_to_number(condition) else { return; };
        if num == 0 {
            self.exit_trap = None;
            return;
        }
        if is_ignored(num) {
            return; // POSIX §2.11: silent no-op
        }
        self.signal_traps.remove(&num);
    }
}
```

### `display_all` union

```rust
pub fn display_all(&self) {
    let (exit_trap, signal_traps) = /* existing saved-traps selection */ ;

    // Exit trap first (unchanged).
    // ...

    // Signal traps union with ignored-on-entry.
    let mut keys: std::collections::BTreeSet<i32> = signal_traps.keys().copied().collect();
    for &sig in crate::signal::ignored_on_entry_set() {
        keys.insert(sig);
    }
    for num in keys {
        let name = Self::signal_number_to_name(num);
        match signal_traps.get(&num) {
            Some(TrapAction::Command(cmd)) => println!("trap -- '{}' SIG{}", cmd, name),
            Some(TrapAction::Ignore) => println!("trap -- '' SIG{}", name),
            Some(TrapAction::Default) => {}
            None => {
                // Ignored-on-entry, no explicit trap set.
                println!("trap -- '' SIG{}", name);
            }
        }
    }
}
```

`BTreeSet` (instead of the current `Vec` + sort) keeps output
deterministically sorted by signal number while deduplicating the
union.

### Parser change for §2.10.2 Rule 5

Insertion point: immediately after the existing `is_valid_name`
check in `parse_for_clause` (currently around
`src/parser/mod.rs:544-552`). The existing check already rejects
numeric or symbol-only tokens; the new check rejects reserved words
specifically.

```rust
// src/parser/mod.rs, parse_for_clause (after is_valid_name branch)

if crate::lexer::reserved::is_posix_reserved_word(name) {
    let span = self.current_span();
    return Err(ShellError::parse(
        ParseErrorKind::UnexpectedToken,
        span.line,
        span.column,
        format!("'{}' is a reserved word and cannot be used as a for-loop variable name", name),
    ));
}
```

`is_posix_reserved_word` is the canonical check delegating to
`lexer::reserved::RESERVED_WORDS` (16 POSIX-reserved words:
`! { } case do done elif else esac fi for if in then until while`).

Note: POSIX §2.4 also lists `time` as a potentially-reserved word
in some implementations, but `lexer::reserved::RESERVED_WORDS`
(the canonical list) does not include it, so `for time in a; do
...` will NOT be rejected by this check. This matches the scope
of `RESERVED_WORDS` (strictly §2.4 fixed reserved words, not
§2.9.1 pipeline-prefix extensions). The test case
`for time in a; do :; done` is therefore expected to **continue
passing** in yosh, aligning with how `time` is currently tokenized
as a plain word.

Updated test inventory drops the `for time` rejection test; see
§Test Inventory below.

## Test Inventory

### Rust integration tests (`tests/ignored_on_entry.rs`, new file)

Each test uses `std::os::unix::process::CommandExt::pre_exec` to
install `SIG_IGN` for specified signals in the forked child before
`exec`ing the yosh binary via `env!("CARGO_BIN_EXE_yosh")`.

| Test name | Scenario | Expectation |
|---|---|---|
| `trap_set_on_ignored_on_entry_sigint_is_silent` | parent INT=SIG_IGN; child: `trap 'echo caught' INT; echo $?` | stdout = `0\n`, exit = 0 |
| `trap_reset_on_ignored_on_entry_sigint_is_silent` | parent INT=SIG_IGN; child: `trap - INT; echo $?` | stdout = `0\n`, exit = 0 |
| `trap_display_shows_ignored_on_entry` | parent TERM=SIG_IGN; child: `trap` | stdout contains `trap -- '' SIGTERM` |
| `subshell_inherits_ignored_on_entry` | parent TERM=SIG_IGN; child: `( trap )` | stdout contains `SIGTERM` |
| `external_cmd_inherits_ignored_on_entry` | parent INT=SIG_IGN; child: `sh -c 'trap' \| grep INT` | exit = 0, stdout non-empty |
| `non_ignored_signal_trap_still_works` | parent no setup; child: `trap 'echo caught' USR1; kill -USR1 $$; wait` | stdout contains `caught` |

### Unit tests

**`src/signal.rs` tests module**:
- `test_ignored_on_entry_is_initially_unset` — before `init_signal_handling`, `is_ignored_on_entry(SIGINT)` returns `false`.
- `test_capture_ignored_on_entry_detects_sig_ign` — install SIG_IGN for a benign signal (e.g. SIGUSR2), call `capture_ignored_on_entry`, assert the set contains it, then restore disposition.
- `test_capture_ignored_on_entry_excludes_default` — for a signal at SIG_DFL, the captured set does not contain it.

These tests acknowledge that `capture_ignored_on_entry` reflects
the *current process* disposition at call time; they are inherently
a process-global operation and must restore state to avoid polluting
sibling tests (mitigated by saving/restoring the original `SigAction`).

**`src/env/traps.rs` tests module**:
- `test_set_trap_with_ignored_predicate_is_silent` — pass
  `&|sig| sig == 2`, call `set_trap_with("INT", Command("x"), ...)`,
  assert `signal_traps` remains empty and result is `Ok(())`.
- `test_remove_trap_with_ignored_predicate_is_silent` — preload
  `signal_traps` with SIGINT=Ignore, call `remove_trap_with("INT",
  &|sig| sig == 2)`, assert the entry is unchanged.
- `test_set_trap_with_non_ignored_predicate_inserts_normally` —
  regression: `&|_| false` behaves like the old `set_trap`.
- `test_set_trap_exit_signal_bypasses_ignored_check` — EXIT (signal 0)
  is always settable regardless of predicate.

**`src/parser/mod.rs` tests module**:
- `parse_for_reserved_word_if_rejected` — `for if in a; do :; done` errors out.
- `parse_for_reserved_word_in_rejected` — `for in in a; do :; done` errors out.
- `parse_for_valid_name_ok` — regression: `for i in a b c; do echo $i; done` parses successfully.
- `parse_for_time_word_ok` — regression: `for time in a; do :; done` continues to parse (POSIX §2.4 reserved set does not include `time`).

### E2E tests (`e2e/posix_spec/2_10_shell_grammar/`, mode 644)

Existing:
- `rule05_for_reserved_word_rejected.sh` — **remove `# XFAIL:` header**, no other change. Test flips from XFAIL to PASS.

New:

| File | Scenario | EXPECT |
|---|---|---|
| `rule05_for_in_word_rejected.sh` | `for in in a; do :; done` | `EXPECT_EXIT: 2`, `EXPECT_STDERR: yosh:` |
| `rule05_for_while_word_rejected.sh` | `for while in a; do :; done` | `EXPECT_EXIT: 2`, `EXPECT_STDERR: yosh:` |
| `rule05_for_valid_name_ok.sh` | `for i in a b c; do echo $i; done` | `EXPECT_OUTPUT: a\nb\nc`, `EXPECT_EXIT: 0` |

### Regression surface

- All 3 existing `e2e/posix_spec/2_11_signals_and_error_handling/*.sh` tests pass (`trap_dash_resets_default`, `trap_exit_runs_on_exit`, `trap_int_by_name`).
- All 13 existing `e2e/posix_spec/2_10_shell_grammar/*.sh` tests pass.
- Full `cargo test --lib` green with ≈ +10 new tests.
- `cargo test --test ignored_on_entry` 6/6 pass.
- `./e2e/run_tests.sh` total count + 3 (new E2E), XFail count −1 (Rule 5 XFAIL promoted), Failed = 0.

## Workflow

### Step 0 — Baseline

```sh
cargo build
cargo test --lib 2>&1 | tail -3
./e2e/run_tests.sh 2>&1 | tail -3
```

Record pre-change counts. Expected (per sub-project 4 close-out):
`Passed: 370, Failed: 0, XFail: 1`. Fail fast if baseline already
regressed.

### Step 1 (Commit ①) — Signal subsystem: capture + skip + union

1. Add `IGNORED_ON_ENTRY` / `capture_ignored_on_entry` /
   `is_ignored_on_entry` / `ignored_on_entry_set` in `src/signal.rs`.
2. Modify `init_signal_handling` to capture first and skip handler
   registration for entry-ignored signals.
3. Modify `reset_child_signals` to union the entry set internally.
4. Add 3 unit tests.
5. Verify:
   ```sh
   cargo test --lib signal::tests
   cargo test --lib
   ./e2e/run_tests.sh
   ```
   Full regression should be unchanged (nothing in production yet
   calls `is_ignored_on_entry` because `TrapStore` changes come in
   Step 2; but `reset_child_signals` change is observable for any
   yosh instance started with a pre-ignored signal — covered by
   Step 3 integration tests).
6. Commit: `feat(signal): capture ignored-on-entry dispositions per POSIX §2.11`.

### Step 2 (Commit ②) — TrapStore silent-ignore + display union

1. Add `set_trap_with` / `remove_trap_with` dependency-injection
   variants; make the existing public methods delegate.
2. Update `display_all` to union `ignored_on_entry_set()` keys and
   render missing entries as `trap -- '' SIG<NAME>`.
3. Replace the existing `Vec<i32>` + sort with `BTreeSet<i32>` for
   deterministic ordering.
4. Add 4 unit tests.
5. Verify:
   ```sh
   cargo test --lib env::traps::tests
   cargo test --lib
   ./e2e/run_tests.sh --filter=trap
   ./e2e/run_tests.sh
   ```
6. Commit: `feat(trap): silent-ignore trap ops on ignored-on-entry signals`.

### Step 3 (Commit ③) — Rust integration tests

1. Create `tests/ignored_on_entry.rs` with `spawn_yosh_with_ignored`
   helper and 6 test cases.
2. Verify:
   ```sh
   cargo build
   cargo test --test ignored_on_entry 2>&1 | tail -10
   ```
   Expected: 6/6 pass.
3. Commit: `test(signal): integration tests for ignored-on-entry inheritance`.

### Step 4 (Commit ④) — §2.10.2 Rule 5 parser rejection + E2E

1. Add `is_reserved_name` helper in `src/parser/mod.rs`
   (delegating to `crate::lexer::reserved::RESERVED_WORDS`).
2. Insert Rule 5 check in `parse_for_clause` after NAME
   extraction.
3. Add 3 parser unit tests.
4. Remove `# XFAIL:` header from
   `e2e/posix_spec/2_10_shell_grammar/rule05_for_reserved_word_rejected.sh`.
5. Add 3 new E2E tests (time / in / valid_ok), `chmod 644`.
6. Verify:
   ```sh
   cargo test --lib parser::tests
   cargo test --lib
   ./e2e/run_tests.sh --filter=rule05
   ./e2e/run_tests.sh 2>&1 | tail -5
   ```
   Expected: `Passed` up by 4 (rule05 XFAIL → PASS plus 3 new),
   `XFail: 0`, `Failed: 0`.
7. Commit: `fix(parser): reject reserved words as for-loop NAME per POSIX §2.10.2 Rule 5`.

### Step 5 (Commit ⑤) — TODO.md cleanup

1. Remove the two closed items from
   `TODO.md` → "Future: POSIX Conformance Gaps (Chapter 2)".
2. Remove the entire section heading (no empty section left behind).
3. Verify:
   ```sh
   grep -n 'POSIX Conformance Gaps' TODO.md   # no output
   ```
4. Commit: `chore(todo): close POSIX Chapter 2 conformance gap section`.

### Commit conventions

- One commit per numbered step. Each commit builds and passes all
  tests at its own tip.
- Commit messages follow project style (`<type>(<scope>): <subject>`).
- Commit body includes the original prompt context: "前回までの記録
  から、 #5 を対応してください" — i.e. this work closes the final
  sub-project (5 of 5) in the POSIX Chapter 2 conformance gap series.

### Verification commands

```sh
cargo build
cargo test --lib
cargo test --test ignored_on_entry
./e2e/run_tests.sh
```

## Success Criteria

1. `trap 'echo x' TERM; kill -TERM $$` in a yosh started with
   SIGTERM=SIG_IGN does NOT print `x` (trap was silently not
   installed).
2. `trap - INT; echo $?` in a yosh started with SIGINT=SIG_IGN
   prints `0` (silent success).
3. `trap` (no args) in a yosh started with SIGTERM=SIG_IGN prints a
   line containing `trap -- '' SIGTERM`.
4. External commands launched via `exec` or as pipeline elements
   inherit the ignore disposition (verified by the `external_cmd_*`
   integration test).
5. `for if in a; do :; done` exits with status 2 and writes a
   `yosh:`-prefixed syntax error to stderr.
6. `for in in a; do :; done` same behaviour.
7. `rule05_for_reserved_word_rejected.sh` passes without `XFAIL`.
8. `cargo test --lib` passes (≈ +10 new tests).
9. `cargo test --test ignored_on_entry` 6/6 pass.
10. `./e2e/run_tests.sh`: `Failed: 0`, `XFail: 0`, total up by 3.
11. `TODO.md` no longer contains the "POSIX Conformance Gaps
    (Chapter 2)" section (the heading itself is gone).
12. 5 commits, each independently building and passing tests.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| `libc::sigaction::sa_sigaction` layout differs between macOS and Linux in ways that affect `== libc::SIG_IGN` comparison | Both platforms use `usize`-sized function pointer for this field; `libc::SIG_IGN` is a sentinel constant in `libc` crate. Add `#[cfg(target_os = "...")]` only if build fails. |
| Stakeholders expect `for time in ...` to error per "bash compatibility" intuition | yosh's POSIX §2.4 `RESERVED_WORDS` does not include `time`. Any future expansion to reject `time` belongs in the `RESERVED_WORDS` list itself — not in this parser check. Out of scope. |
| `init_signal_handling` being called from multiple entry points could double-capture | `OnceLock::get_or_init` ensures capture happens exactly once per process. |
| Interactive shell started with SIGINT=SIG_IGN loses Ctrl+C functionality | This is a POSIX-mandated consequence and matches bash/dash. Rare in practice. Accept as correct behaviour. |
| `display_all` union changes output ordering for existing E2E `trap_*.sh` tests | `BTreeSet` preserves sort-by-signal-number order; with empty ignored-on-entry set the union is a no-op. Verified in Step 2 by `./e2e/run_tests.sh --filter=trap`. |
| `is_reserved_name` rejects `time` but user scripts rely on bash behaviour | yosh's policy prioritises POSIX compliance. Document in `CLAUDE.md` if user requests; this sub-project does not add such documentation. |
| Rust integration test `spawn_yosh_with_ignored` relies on `pre_exec` being `unsafe` | Standard Rust idiom; `CommandExt::pre_exec` requires `unsafe` because the closure runs in post-fork, pre-exec context where async-signal-safety rules apply. Only `sigaction` (async-signal-safe) is called inside. |
| `capture_ignored_on_entry` unit test pollutes process signal state | Tests save original `SigAction` before modifying and restore it afterwards; tests are serial by default within a test module but parallel across modules — isolate via careful scope. |
| `SaveTraps`/`reset_for_command_sub` snapshot may need to also snapshot ignored-on-entry for `$(trap)` accuracy | `IGNORED_ON_ENTRY` is process-global and never changes, so `$(trap)` inside command substitution sees the same set. No snapshot needed. |

## Out of Scope (explicit)

- §2.10.2 Rule 8 (NAME in function) — same bug class, tracked as
  future work.
- POSIX §2.4 `time` keyword support as a pipeline prefix (orthogonal).
- ERR / DEBUG / RETURN pseudo-traps (bash extensions).
- `trap -l` (list signals) builtin extension.
- Interactive vs non-interactive behavioural divergence for
  ignored-on-entry (decided: both preserve).
- SIGWINCH / SIGINFO / other non-POSIX signals.
