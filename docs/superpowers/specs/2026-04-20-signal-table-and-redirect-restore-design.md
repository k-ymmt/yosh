# Design: Signal Table Portability & Redirect Error Restore

**Date:** 2026-04-20
**Scope:** Two targeted correctness bugs identified in TODO.md
**Status:** Draft — pending user review

## Background

Two pre-existing bugs surfaced while triaging TODO.md:

- **B.** `src/signal.rs` hard-codes Linux signal numbers in `SIGNAL_TABLE` and `HANDLED_SIGNALS`. Several signals have different numbers on macOS, so name↔number lookups are wrong for macOS binaries. This affects `trap`, `kill -l`, and `capture_ignored_on_entry` (POSIX §2.11 inherited-SIG_IGN tracking).
- **C.** `src/exec/simple.rs` has four call sites where `RedirectState::apply(...)` is invoked with `save=true`. If `apply` fails partway through a multi-redirect list, the already-saved `(original_fd, saved_fd)` pairs are left in the state, and the early `return Err(...)` does not call `restore()`. The `Drop` impl only closes saved copies without dup2-ing them back, so target fds stay corrupted after the error.

## Goals

- Signal name↔number lookups return correct results on both Linux and macOS.
- After any `RedirectState::apply` call (whether it returned `Ok` or `Err`), the shell's fd table is in a consistent state: either fully applied (Ok) or fully rolled back (Err).
- No regressions in existing signal/redirect behaviour.

## Non-Goals

- Adding support for signals not currently in `SIGNAL_TABLE` (e.g. SIGBUS, SIGIO, real-time signals). Out of scope.
- Changing the `RedirectState` API shape (`apply` / `restore` / `Drop`). We only change internal behaviour on the error path.
- Reviewing non-simple-command redirect paths (pipelines, compound commands). Audit noted but fix is scoped to `src/exec/simple.rs`.

## B. Signal Table Portability

### Root Cause

| Name     | Linux | macOS | In `SIGNAL_TABLE` |
|----------|-------|-------|-------------------|
| SIGUSR1  | 10    | 30    | `(10, "USR1")`    |
| SIGUSR2  | 12    | 31    | `(12, "USR2")`    |
| SIGCHLD  | 17    | 20    | `(17, "CHLD")`    |
| SIGCONT  | 18    | 19    | `(18, "CONT")`    |
| SIGSTOP  | 19    | 17    | `(19, "STOP")`    |
| SIGTSTP  | 20    | 18    | `(20, "TSTP")`    |

On macOS, `SIGBUS=10` and `SIGSYS=12`. A macOS parent that ignored `SIGBUS` would be observed as "ignoring USR1" by `capture_ignored_on_entry`, which walks `SIGNAL_TABLE` and reports the name side. Same inversion for CHLD↔TSTP, CONT↔STOP.

`HANDLED_SIGNALS` (`src/signal.rs:41-49`) has the same Linux literals for HUP/INT/QUIT/ALRM/TERM/USR1/USR2. HUP/INT/QUIT/ALRM/TERM are identical across Linux and macOS, but USR1/USR2 cause the shell to register handlers for the wrong kernel signal on macOS.

### Design

Replace every numeric literal in `SIGNAL_TABLE` and `HANDLED_SIGNALS` with the corresponding `libc::SIG*` constant. The `libc` crate exposes these as `pub const c_int`, so they are usable in a `const SIGNAL_TABLE: &[(i32, &str)]` context.

```rust
pub const SIGNAL_TABLE: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGABRT, "ABRT"),
    (libc::SIGKILL, "KILL"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
    (libc::SIGPIPE, "PIPE"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGCHLD, "CHLD"),
    (libc::SIGCONT, "CONT"),
    (libc::SIGSTOP, "STOP"),
    (libc::SIGTSTP, "TSTP"),
    (libc::SIGTTIN, "TTIN"),
    (libc::SIGTTOU, "TTOU"),
];

pub const HANDLED_SIGNALS: &[(i32, &str)] = &[
    (libc::SIGHUP, "HUP"),
    (libc::SIGINT, "INT"),
    (libc::SIGQUIT, "QUIT"),
    (libc::SIGALRM, "ALRM"),
    (libc::SIGTERM, "TERM"),
    (libc::SIGUSR1, "USR1"),
    (libc::SIGUSR2, "USR2"),
];
```

### Testing

Existing tests (`test_signal_name_to_number_*`, `test_signal_number_to_name_*`, `test_handled_signals_are_in_signal_table`) continue to pass: they assert name/number pairs that happen to be portable on x86_64 Linux (which matches the original literals). Since `libc::SIGHUP == 1` etc. on Linux, these tests remain green on Linux CI.

Add one new unit test that pins the fix intent platform-independently:

```rust
#[test]
fn test_signal_table_matches_libc_constants() {
    // Portable check: the table must agree with libc on every entry.
    // Pre-fix this would have failed on macOS for USR1/USR2/CHLD/CONT/STOP/TSTP.
    for &(num, name) in SIGNAL_TABLE {
        let expected = match name {
            "HUP" => libc::SIGHUP,
            "INT" => libc::SIGINT,
            "QUIT" => libc::SIGQUIT,
            "ABRT" => libc::SIGABRT,
            "KILL" => libc::SIGKILL,
            "USR1" => libc::SIGUSR1,
            "USR2" => libc::SIGUSR2,
            "PIPE" => libc::SIGPIPE,
            "ALRM" => libc::SIGALRM,
            "TERM" => libc::SIGTERM,
            "CHLD" => libc::SIGCHLD,
            "CONT" => libc::SIGCONT,
            "STOP" => libc::SIGSTOP,
            "TSTP" => libc::SIGTSTP,
            "TTIN" => libc::SIGTTIN,
            "TTOU" => libc::SIGTTOU,
            other => panic!("unexpected signal name in table: {other}"),
        };
        assert_eq!(
            num, expected,
            "SIGNAL_TABLE entry for {name} has {num}, libc says {expected}"
        );
    }
}
```

This test would have caught the bug on macOS CI pre-fix.

### Risk

Essentially zero. `libc::SIG*` values are resolved at compile time on each target platform and are the canonical source. The change is a pure refactor in terms of semantics on Linux (identical values) and a bug fix on macOS.

## C. RedirectState Self-Healing apply()

### Root Cause

`RedirectState::apply` iterates redirects in order:

```rust
pub fn apply(&mut self, redirects: &[Redirect], env: &mut ShellEnv, save: bool) -> Result<(), String> {
    for redirect in redirects {
        self.apply_one(redirect, env, save)?;   // <-- early return on failure
    }
    Ok(())
}
```

If the second redirect fails, the first has already pushed `(orig, saved)` into `self.saved_fds` and dup2'd a new fd over the target. The caller in `src/exec/simple.rs` (four sites: fg/bg/jobs at 240-244, command at 270-274, Special-builtin at 322-325, Regular-builtin at 344-349) returns `Err(...)` without calling `restore()`. The `RedirectState` then goes out of scope and `Drop` runs:

```rust
impl Drop for RedirectState {
    fn drop(&mut self) {
        for (_original, saved) in self.saved_fds.drain(..) {
            unsafe { libc::close(saved) };
        }
    }
}
```

`Drop` closes saved copies but does **not** `dup2(saved, original)` to restore. Net effect: the shell's fd 1 (or whatever target) stays pointing at whatever the first redirect opened, silently breaking subsequent output until the shell exits or another redirect happens to clobber it.

### Design

Make `apply` self-healing: on failure, roll back any redirects already applied within that call.

```rust
pub fn apply(&mut self, redirects: &[Redirect], env: &mut ShellEnv, save: bool) -> Result<(), String> {
    for redirect in redirects {
        if let Err(e) = self.apply_one(redirect, env, save) {
            // Roll back any partially-applied redirects from this call.
            // save=false leaves saved_fds empty, so this is a no-op for exec.
            self.restore();
            return Err(e);
        }
    }
    Ok(())
}
```

**Contract after this change:** `apply` leaves `self` in a consistent state on both return paths.
- `Ok(())`: all redirects applied, `saved_fds` populated (caller must eventually `restore`).
- `Err(_)`: no redirects applied, `saved_fds` empty, fd table restored to pre-apply state.

**Save=false (exec no-args path):** `saved_fds` stays empty because `save_fd` is never called. `restore()` iterates an empty vec — no-op. The exec path's partial-application semantics are unchanged (new fds that were opened before the failing one remain leaked, same as today; this is a pre-existing issue outside scope).

**Why not fix at call sites (Option 1 rejected):** Four call sites today, likely more as the shell grows (pipelines, compound commands). The rule "every caller must call restore() on error" is easy to forget. Encapsulating the invariant inside `apply` removes the footgun.

### Testing

Add a unit test in `src/exec/redirect.rs`:

```rust
#[test]
fn test_apply_rolls_back_on_second_redirect_failure() {
    // Arrange: two redirects, second targets a non-existent directory so open() fails.
    let mut env = make_env();
    let tmp_ok = std::env::temp_dir().join("yosh_apply_rollback_ok.txt");
    let bad_path = "/no/such/dir/should-not-exist/file.txt";

    let redirects = vec![
        Redirect {
            fd: Some(1),
            kind: RedirectKind::Output(Word::literal(tmp_ok.to_str().unwrap())),
        },
        Redirect {
            fd: Some(2),
            kind: RedirectKind::Output(Word::literal(bad_path)),
        },
    ];

    // Save original fd 1 outside the RedirectState so we can verify it still
    // refers to the console after apply fails.
    let orig_stdout = unsafe { libc::dup(1) };
    assert!(orig_stdout >= 0);

    let mut state = RedirectState::new();
    let err = state.apply(&redirects, &mut env, true);
    assert!(err.is_err(), "second redirect must fail");

    // Post-condition: saved_fds is empty (rollback happened).
    assert!(state.saved_fds.is_empty(), "saved_fds should be empty after rollback");

    // Post-condition: fd 1 is restored — writing to it should NOT land in tmp_ok.
    let marker = b"post-rollback\n";
    unsafe { libc::write(1, marker.as_ptr() as *const _, marker.len()); }

    let written = std::fs::read_to_string(&tmp_ok).unwrap_or_default();
    assert!(
        !written.contains("post-rollback"),
        "fd 1 should not still point at tmp_ok after rollback; got: {written:?}"
    );

    // Clean up
    unsafe { libc::dup2(orig_stdout, 1); libc::close(orig_stdout); }
    let _ = std::fs::remove_file(&tmp_ok);
}
```

Visibility note: `saved_fds` is currently a private field. The test lives in the same module (`#[cfg(test)] mod tests` inside `redirect.rs`), so it has access. No public API changes required.

### Risk

Low. The change is confined to `apply`'s error path — the happy path is byte-for-byte identical. Callers already tolerate `apply` returning `Err`; they just skip `restore()` in that case. After the fix they would still skip `restore()`, which is now correct (state is already clean).

One subtle risk: if a future contributor calls `restore()` after a failed `apply()`, it becomes a double-restore on an empty `saved_fds` — harmless no-op. No regression possible.

## Cross-Cutting Concerns

- **Commit granularity:** Two separate commits (B, then C), each with its own test. Keeps `git log` searchable and the blast radius per commit tight.
- **CLAUDE.md & TODO.md:** Remove the two bullet points from `TODO.md` as part of the C commit (B's bullet as part of the B commit), per the project convention "delete completed items."
- **CI coverage:** If CI runs only on Linux, the B test is still valuable (it pins the libc-constant invariant). Consider scheduling a macOS check via GitHub Actions separately — tracked in TODO.md if not already.

## Out of Scope (Noted for Future TODO.md entries)

- Auditing non-`src/exec/simple.rs` callers of `RedirectState::apply`. If the compound-command / pipeline executor has a similar pattern, it is now protected by the self-healing `apply`, but explicit audit is recommended.
- Extending `SIGNAL_TABLE` with SIGBUS, SIGIO, SIGPROF, SIGWINCH, etc. for `trap`/`kill -l` completeness.
- Making `RedirectState::saved_fds` non-private or introducing a `is_clean()` helper for cross-module tests.

## Implementation Order

1. B (signal table) — smaller, isolated, verifies tooling.
2. C (redirect self-heal) — builds on a green build.

Each step: code change → new test → `cargo test` → `cargo fmt --edition 2024 --check <path>` → commit.
