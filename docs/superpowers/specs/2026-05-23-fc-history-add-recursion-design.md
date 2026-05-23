# `fc` Builtin Stack-Overflow on Self-Reference — Design

**Date:** 2026-05-23
**Status:** Design
**Type:** Bug fix / POSIX conformance
**Closes TODO entry:** `TODO.md` §SP6 follow-ups — "`fc` builtin with no operand (`fc`, `fc -e <editor>`) infinite-recurses and stack-overflows yosh"

## 1. Background

`Repl::run` (`src/interactive/mod.rs:268-272`) appends the running
command to history **before** calling `exec_complete_command`:

```rust
let cmd_text = input_buffer.trim_end().to_string();
self.executor
    .env
    .history
    .add(&cmd_text, histsize, &histcontrol);

for cmd in &commands {
    let status = self.executor.exec_complete_command(cmd);
    ...
}
```

When the user types bare `fc` (or `fc -e <editor>`), the sequence is:

1. `history.add("fc", ...)` — `"fc"` becomes the most recent entry.
2. `exec_complete_command` invokes `builtin_fc` (`src/builtin/special.rs:589`).
3. With zero operands, `fc_resolve_range` returns `(hist_len - 1, hist_len - 1)`
   (`src/builtin/special.rs:680-687`) — i.e. the most recent entry, which is
   `"fc"` itself.
4. `fc_edit` opens an editor on a tempfile containing `"fc"` and, after
   the editor exits, calls `executor.eval_string("fc")`.
5. `eval_string` re-enters `builtin_fc`. The history is unchanged
   (because `eval_string` does not route through `Repl::run`), so step 3
   resolves to `"fc"` again. Each recursion grows the Rust call stack
   inside `exec_complete_command` → eventually SIGSEGV (stack overflow).

POSIX XCU §fc RATIONALE specifies that `fc` itself "shall not be
entered into the history list", so the upstream cause is "yosh records
fc in history before it runs", and the fix lies in `Repl::run`, not in
the `fc` builtin.

Existing tests in `tests/pty_posix.rs::fc` (`editor_dash_e`,
`no_args_uses_editor`) and `tests/pty_posix.rs::fcedit`
(`used_by_fc`, `default_ed`) work around the bug by passing the
prefix-match operand `echo`, which forces `fc_resolve_one` to skip the
fc command and select a seeded `echo seedline` entry. Those workarounds
exercise only a subset of the bug's surface — the bare-invocation paths
(`fc`, `fc -e cat`, `fc -s ...`) remain uncovered.

## 2. Scope

**In scope:**

- Hoist the `history.add` call in `Repl::run` from before
  `exec_complete_command` to after the inner `for cmd in &commands`
  loop (`src/interactive/mod.rs:251-294`).
- Remove the `echo` prefix-match workaround from four PTY tests:
  - `tests/pty_posix.rs::fc::editor_dash_e`
  - `tests/pty_posix.rs::fc::no_args_uses_editor`
  - `tests/pty_posix.rs::fcedit::used_by_fc`
  - `tests/pty_posix.rs::fcedit::default_ed`
- Add one regression test asserting bare `fc` (FCEDIT=cat, single
  seeded history entry) exits 0 without stack-overflow:
  `tests/pty_posix.rs::fc::bare_fc_does_not_recurse`.
- Remove the resolved item from `TODO.md`.

**Out of scope:**

- `fc_substitute`'s own `history.add` call at
  `src/builtin/special.rs:857` — preserved because it adds the
  **substituted** command (e.g. `echo new`), not the fc invocation
  itself. POSIX permits this; bash exhibits the same behaviour.
- Filtering fc commands from history entirely (POSIX-strict reading
  of "shall not be entered into the history list"). Doing so would
  also remove fc from up-arrow navigation, which surprises users
  more than it benefits them. yosh accepts the deviation; see §5.
- The synthesis logic of multi-line history entries — unchanged.
- The `history` builtin itself (yosh does not implement one).

## 3. Architecture

### 3.1 Current flow (`src/interactive/mod.rs`)

```
read line
└── ParseStatus::Complete(commands)
    ├── history.add(cmd_text)        ← BEFORE exec
    ├── for cmd in commands: exec_complete_command(cmd)
    └── input_buffer.clear()
```

### 3.2 Proposed flow

```
read line
└── ParseStatus::Complete(commands)
    ├── for cmd in commands: exec_complete_command(cmd)
    ├── history.add(cmd_text)        ← AFTER exec
    └── input_buffer.clear()
```

The `for` loop's early-break on `exit_requested.is_some()` still
falls through to `history.add` (the new placement is **after** the
loop, so a `break` exits to the add call). This preserves the
existing behaviour of recording `exit` in history before the REPL
itself exits.

### 3.3 Why this works for the fc case

After the move, when `builtin_fc` runs:

- The history slice it observes (`executor.env.history.entries()`)
  does **not** include the fc command itself — that entry is added
  later, after `exec_complete_command` returns.
- `fc_resolve_range` with zero operands resolves to the **previous**
  user command (the one fc was meant to operate on), exactly as the
  POSIX fc semantics describe.
- `eval_string` on the edited content runs the previous command, not
  fc. No recursion.

## 4. Implementation

### 4.1 `src/interactive/mod.rs` — diff sketch

```rust
ParseStatus::Complete(commands) => {
    let histsize: usize = ... ;       // unchanged
    let histcontrol = ... ;           // unchanged
    let cmd_text = input_buffer.trim_end().to_string();
    // REMOVED: self.executor.env.history.add(&cmd_text, histsize, &histcontrol);

    for cmd in &commands {
        let status = self.executor.exec_complete_command(cmd);
        self.executor.env.exec.last_exit_status = status;
        if self.executor.exit_requested.is_some() {
            break;
        }
    }

    // ADDED: history.add after execution so fc's "previous command"
    // resolves to the user's prior input, not fc itself.
    self.executor
        .env
        .history
        .add(&cmd_text, histsize, &histcontrol);

    input_buffer.clear();
}
```

### 4.2 Test updates — `tests/pty_posix.rs`

Drop the `echo` operand and the multi-paragraph workaround comment
from the four affected tests. Example for `editor_dash_e`:

```rust
#[test]
fn editor_dash_e() {
    let (mut session, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    run_and_drain(&mut session, "echo seedline");

    let out = capture_until_sentinel(
        &mut session,
        "fc -e cat </dev/null >/dev/null 2>&1; echo RC=$?",
    );

    assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

    session.send_line("exit").unwrap();
    let _ = session.expect(Eof);
}
```

The same pattern applies to `no_args_uses_editor`, `used_by_fc`, and
`default_ed`: drop the `echo` operand from the fc command line, drop
the workaround comment, keep everything else.

### 4.3 New regression test

Add to `mod fc` in `tests/pty_posix.rs`:

```rust
#[test]
fn bare_fc_does_not_recurse() {
    // Regression: bare `fc` with FCEDIT=cat used to stack-overflow
    // because Repl::run added the fc command to history BEFORE running
    // it, so fc's "previous command" resolved to itself. Hoisting
    // history.add to after exec_complete_command fixes this.
    let (mut session, _tmpdir) = spawn_yosh();
    wait_for_prompt(&mut session);

    run_and_drain(&mut session, "export FCEDIT=cat");
    run_and_drain(&mut session, "echo seedline");

    let out = capture_until_sentinel(
        &mut session,
        "fc </dev/null >/dev/null 2>&1; echo RC=$?",
    );

    assert!(out.contains("RC=0"), "expected RC=0 in: {:?}", out);

    session.send_line("exit").unwrap();
    let _ = session.expect(Eof);
}
```

### 4.4 TODO.md

Remove the bullet at line 221-231 (the `fc` infinite-recursion entry
under SP6 follow-ups).

## 5. Behavioural changes & their justification

| Scenario | Before | After | Justification |
|---|---|---|---|
| Bare `fc` (default editor) | Stack overflow (SIGSEGV) | Re-executes previous command per POSIX | The bug we're fixing |
| `fc -e <editor>` (no operand) | Stack overflow | Re-executes previous command | Same |
| `fc -l` (list) | Lists history (fc inclusive) | Lists history (fc excluded — not yet added) | Closer to POSIX; previously fc would appear once printed-before-add |
| `fc -s old=new` (substitute) | History: `[..., fc_cmd, substituted]` | History: `[..., substituted, fc_cmd]` | Substituted command is what the user "ran"; reordering matches semantic recency |
| `exit` | Recorded in history before shell exits | Recorded in history before shell exits (via post-loop add) | Unchanged |
| `Ctrl-C` mid-command | Recorded in history | Recorded in history | Unchanged — SIGINT doesn't set `exit_requested` |
| Panic during exec (theoretical) | Recorded in history | Not recorded (unwinds past history.add) | Acceptable — yosh treats panic-during-exec as a crash, not a normal exit |

The `fc -s` history-ordering change is the only user-visible deviation
from current behaviour for a working command. It is acceptable because:

- POSIX does not specify the order in which `fc` and the commands it
  produces appear in history.
- The substituted command is semantically what the user just ran; up-arrow
  surfacing the substituted command before the `fc -s` invocation
  matches the mental model of "I ran X, so X is my most recent command".
- Different shells handle this differently and POSIX is silent; yosh
  picks the ordering that falls out naturally from the simpler fix.

## 6. Testing

**Unit tests:** none required — the change is structural (statement
reordering inside one function with no new branching) and is exercised
end-to-end by the PTY tests.

**Integration tests:**

- 4 updated PTY tests (drop workaround) — they continue to PASS.
- 1 new PTY test (`bare_fc_does_not_recurse`) — covers the actual
  recursion case for the first time.
- Existing `mod fc::{list_recent, list_no_numbers, list_reverse,
  substitute}` and `mod fcedit::default_ed` — verified unchanged
  behaviour (no workaround dependency).

**Manual verification (smoke):**

- Run `FCEDIT=cat cargo run` interactively, type `echo foo`, then bare
  `fc`. Confirm yosh prints `echo foo` (the previous command, surfaced
  by `cat` as the no-op editor) and re-executes it, rather than
  stack-overflowing.
- Type `exit`, confirm history file (`$HISTFILE`) contains `exit`
  as the final entry.

## 7. Rollout

Single PR. No backwards-compatibility considerations (yosh is
pre-1.0; interactive shell history ordering is not part of any
documented API).

## 8. Files touched

- `src/interactive/mod.rs` — hoist `history.add` (4-line change)
- `tests/pty_posix.rs` — update 4 tests + add 1 regression test
- `TODO.md` — remove the resolved SP6 follow-up bullet

## 9. Risk

**Low.** The change is a 4-line reordering inside one function. The
new code path is exercised by both updated and new PTY tests. The only
behavioural change for non-fc commands is the `fc -s` history ordering
(§5), which is below the threshold of "user-visible regression".
