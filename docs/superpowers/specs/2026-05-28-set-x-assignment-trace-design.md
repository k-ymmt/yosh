# `set -x` Assignment-Only Trace — Design

**Date:** 2026-05-28
**Status:** Approved (brainstorming)
**POSIX_REF:** XCU 2.5.3 (Shell Variables, execution trace); `set` builtin

## 1. Problem

A command consisting only of variable assignments (no command name), e.g.
`x=1` or `a=1 b=2`, emits **nothing** under `set -x`. Both reference
shells trace each assignment on its own line:

```sh
$ bash -c 'set -x; a=1 b=2'
+ a=1
+ b=2
$ dash -c 'set -x; a=1 b=2'
+ a=1
+ b=2
```

This is the only `set -x` case where yosh diverges from **both** bash
and dash. The PS4 full-support spec (2026-05-28) explicitly scoped trace
coverage out (§6); this spec closes the unambiguous portion of that
follow-up.

### Empirical survey (informational)

Probed 2026-05-28 on macOS bash 3.2 / dash / yosh HEAD:

| Case | yosh (now) | bash | dash |
|---|---|---|---|
| `x=1` (assignment-only) | **(nothing)** | `+ x=1` | `+ x=1` |
| pipeline `echo a \| cat` | `+ echo a` / `+ cat` | same | same |
| `for i in 1 2; do echo $i; done` | body only | `+ for i in 1 2` per-iter + body | body only |
| `while`/`if` | cond + body | same | same |
| `case` | body only | `+ case x in` + body | body only |
| and-or, subshell | each cmd | same | same |
| `x=$(echo hi)` | `+ echo hi` (no `++`) | `++ echo hi` + `+ x=hi` | same as bash |

Findings:
- yosh **already traces** the simple commands inside compound bodies and
  pipeline members (they route through `exec_simple_command`). The TODO
  entry that motivated this work overstated the gap.
- `for` / `case` structural headers (bash-only, dash omits) display the
  **source word text** re-quoted, not the expanded value. Matching bash
  exactly requires Word→source rendering plus an xtrace quoting
  algorithm. POSIX leaves the header format implementation-defined, so
  yosh's current dash-parity behaviour is conformant.
- `++` replication for command substitutions inside an assignment value
  is intentionally *not* added — consistent with the PS4 spec §2
  decision to not count command substitutions in `indirection_level`.

## 2. Decisions

- **Scope: assignment-only trace.** Close the cross-shell divergence.
  Leave `for`/`case` headers and argument quoting as separate concerns
  (deferred TODO entries).
- **One trace line per assignment.** `a=1 b=2` emits `+ a=1` and
  `+ b=2` on separate lines (matches both bash and dash).
- **Display the expanded value, unquoted.** Consistent with yosh's
  existing simple-command trace style (`expanded.join(" ")` with no
  per-arg quoting). Argument quoting is a separate cross-cutting
  concern affecting simple-command traces too.
- **Trace after expansion, before the `set` call.** Matches bash's
  ordering: nested command-sub traces (`+ echo hi`) appear first, then
  the assignment trace (`+ x=hi`).
- **Reuse `xtrace_prefix` per emitted line.** PS4 expansion (parameter
  / arithmetic / command sub) and first-character replication are
  already correct from the PS4 full-support work; no new code in the
  prefix builder. Calling once per assignment matches bash semantics
  (PS4 with `$(date)` evaluates per trace line).

## 3. Architecture

Single insertion point in `src/exec/simple.rs`, inside the
assignment-only branch (currently lines 188–225). Compound, pipeline,
and `exec_simple_command`'s expanded-command path are untouched.

### 3.1 Trace insertion

```rust
for assignment in &cmd.assignments {
    let has_cmd_sub = assignment.value.as_ref().is_some_and(word_has_command_sub);
    let value = match assignment.value.as_ref() {
        Some(w) => match crate::expand::expand_word_to_string(&mut self.env, w) {
            Ok(v) => v,
            Err(e) => { /* existing early-return */ }
        },
        None => String::new(),
    };

    // NEW: emit trace after expansion, before set_with_options.
    if self.env.mode.options.xtrace {
        let prefix = xtrace_prefix(&mut self.env);
        eprintln!("{}{}={}", prefix, assignment.name, value);
    }

    if has_cmd_sub { last_cmd_sub_status = Some(self.env.exec.last_exit_status); }
    if let Err(e) = self.env.vars.set_with_options(/* ... */) { /* existing */ }
}
```

### 3.2 Ordering rationale

- **Expansion → trace → set.** A command substitution inside the value
  emits its own trace during `expand_word_to_string`; the assignment
  trace follows. yosh does not promote to `++` because
  `indirection_level` is unchanged for command substitutions (PS4 spec
  §2).
- **Per-assignment iteration.** Each pass through the loop emits one
  line. Multi-assignment ordering: `a=1 b=2` → `+ a=1` then `+ b=2`,
  matching bash and dash.
- **`xtrace_prefix` called per line.** Matches the existing simple-
  command trace site (`simple.rs:228-231`) — one call per trace line.
  Side effect: a PS4 containing `$(cmd)` evaluates per trace line,
  matching bash.

## 4. Error handling

- **Value expansion failure** (`expand_word_to_string` returns `Err`):
  existing early-return path runs; the assignment is **not** traced.
  Matches bash (no trace for an aborted expansion).
- **`set_with_options` failure** (readonly variable, etc.): trace has
  already been emitted; the existing early-return path then fires. The
  trace line precedes the error message on stderr. Matches bash.

## 5. Out of scope

- **`for` / `case` structural headers** (bash extension; dash omits;
  POSIX leaves implementation-defined). Adding bash-style headers would
  require Word→source rendering and an xtrace quoting algorithm. Stays
  as a deferred TODO.
- **Argument quoting in trace output** (`echo "a b" c` → bash
  `+ echo 'a b' c` vs yosh `+ echo a b c`). Affects existing simple-
  command traces too; cross-cutting concern, separate work.
- **`++` replication for nested command substitutions.** Already
  deliberately omitted per PS4 spec §2.
- **Pipeline / compound body / function call tracing.** Already correct
  (routes through `exec_simple_command`).

## 6. Testing

### Unit (`src/exec/simple.rs::tests`)

The existing `xtrace_prefix` test group is unchanged (signature is
unchanged). New tests capture stderr via a redirect-based helper or
process-substitution pattern already used in the crate; if no pattern
exists, restrict unit tests to direct invariants (the assignment
branch's flow) and cover the stderr output via the E2E tier.

Minimum unit coverage:
- An assignment-only command with `xtrace=false` produces no trace
  side effect (existing behaviour preserved).
- The assignment-only branch returns `Ok(0)` for a single assignment
  with no command sub when `xtrace=true` (verifies the trace block
  does not change control flow).

### E2E (`e2e/posix_spec/4_special_builtin/`, 644 perms)

Placement is settled: the existing `set_opt_x_traces.sh` and
`set_opt_o_xtrace_alias.sh` live here with `POSIX_REF: 2.14.7 set`.
New tests follow the same header format (`EXPECT_STDERR:` for trace
assertions) and the `set_opt_x_<aspect>.sh` naming pattern.

- `set_opt_x_assign.sh` — `set -x; x=1` → stderr has `+ x=1`.
- `set_opt_x_multi_assign.sh` — `set -x; a=1 b=2` → stderr has
  `+ a=1` and `+ b=2` on **separate lines**, in that order.
- `set_opt_x_assign_empty.sh` — `set -x; x=` → stderr has `+ x=`
  (trailing `=`, empty value).
- `set_opt_x_assign_cmdsub.sh` — `set -x; x=$(echo hi)` → stderr has
  `+ echo hi` before `+ x=hi` (order matters).
- `set_opt_x_assign_ps4.sh` — `PS4='> '; set -x; x=1` → stderr has
  `> x=1` (PS4 prefix applies).

Existing PS4 e2e tests (`PS4_assigned.sh`, `PS4_default.sh`,
`PS4_expansion.sh`, `PS4_nesting.sh`, `PS4_dot_nesting.sh`) must stay
green unchanged.

## 7. Affected files

- `src/exec/simple.rs` — insert ~4-line trace block inside the
  assignment-only branch; add 1-2 unit tests if a stderr-capture
  pattern is feasible.
- `e2e/posix_spec/.../set_x_assign*.sh` — 5 new E2E files (644 perms).
- `TODO.md`:
  - Revise the existing "set -x trace coverage is simple-commands-only"
    line under "Future: Interactive Mode Enhancements". New text
    narrows the scope to the still-deferred items only:

    > `set -x` does not emit bash-style structural headers for `for` /
    > `case` (yosh matches dash here; POSIX leaves the header format
    > implementation-defined). Adding bash parity requires
    > Word→source rendering plus xtrace argument quoting, the latter
    > of which also affects existing simple-command trace output
    > (`echo "a b" c` traces as `+ echo a b c` not `+ echo 'a b' c`).
    > Tracked together because both want the same quoting helper.
    > See `docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md`
    > §5 for the assignment-only portion that has been closed.
    > (`src/exec/compound.rs`, `src/exec/simple.rs`)

## 8. Acceptance criteria

- `bash -c 'set -x; a=1 b=2' 2>&1` and `yosh -c 'set -x; a=1 b=2' 2>&1`
  produce equivalent assignment-trace lines (PS4 default `+ `).
- `cargo test` passes.
- `./e2e/run_tests.sh` passes, including the five new tests above and
  the unchanged PS4 e2e tests.
- TODO.md reflects the closed scope and explicitly preserves the
  deferred header / quoting items.
