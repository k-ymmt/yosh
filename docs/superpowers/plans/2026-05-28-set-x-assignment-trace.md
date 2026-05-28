# `set -x` Assignment-Only Trace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `set -x` emit `+ name=value` for assignment-only commands (e.g. `x=1`, `a=1 b=2`), closing the only `set -x` divergence between yosh and both bash/dash. Multi-assignment emits one line per assignment.

**Architecture:** Insert a 4-line trace block into the existing assignment-only branch of `exec_simple_command` (`src/exec/simple.rs`), positioned **after** value expansion (so any nested command-sub trace fires first) and **before** `set_with_options`. Reuses the existing `xtrace_prefix` (PS4 expansion + first-char replication from the 2026-05-28 PS4 spec). No changes to compound/pipeline execution — they already trace via `exec_simple_command`.

**Tech Stack:** Rust (yosh executor), shell-based E2E test framework (`e2e/run_tests.sh`).

**Spec:** `docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md`

---

## File Structure

- **Modify:** `src/exec/simple.rs`
  - Insert trace block in assignment-only branch (current lines ~188–225, between value computation and `set_with_options`).
  - Add one unit test in the `#[cfg(test)] mod tests` section for control-flow regression.
- **Create:** 5 new E2E test files (644 perms each) under `e2e/posix_spec/4_special_builtin/`:
  - `set_opt_x_assign.sh`
  - `set_opt_x_multi_assign.sh`
  - `set_opt_x_assign_empty.sh`
  - `set_opt_x_assign_cmdsub.sh`
  - `set_opt_x_assign_ps4.sh`
- **Modify:** `TODO.md` — revise the "set -x trace coverage is simple-commands-only" entry to reflect closed assignment scope and the deferred for/case header + quoting concerns.

**E2E assertion strategy:** `EXPECT_STDERR` uses substring matching (`e2e/run_tests.sh:383` — `*"$meta_expect_stderr"*`). Each test picks a substring that uniquely proves the new behaviour (e.g. `+ b=2` proves multi-assignment per-line behavior because if the trace combined into `+ a=1 b=2`, the leading `+ ` would not appear before `b=2`).

---

### Task 1: Write the failing E2E tests

Five small shell scripts. Project convention (see commits `51805b9`, `e8de7fd`) is to batch e2e tests in one commit when they target the same feature. All five tests fail at this stage because no trace is emitted for assignment-only commands.

**Files:**
- Create: `e2e/posix_spec/4_special_builtin/set_opt_x_assign.sh`
- Create: `e2e/posix_spec/4_special_builtin/set_opt_x_multi_assign.sh`
- Create: `e2e/posix_spec/4_special_builtin/set_opt_x_assign_empty.sh`
- Create: `e2e/posix_spec/4_special_builtin/set_opt_x_assign_cmdsub.sh`
- Create: `e2e/posix_spec/4_special_builtin/set_opt_x_assign_ps4.sh`

- [ ] **Step 1: Create `set_opt_x_assign.sh`** (single assignment)

```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces a single assignment-only command as + name=value
# EXPECT_STDERR: + x=1
# EXPECT_EXIT: 0
set -x
x=1
```

- [ ] **Step 2: Create `set_opt_x_multi_assign.sh`** (multi-assignment, one line per assignment)

```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces each assignment on its own line (matches bash/dash)
# EXPECT_STDERR: + b=2
# EXPECT_EXIT: 0
set -x
a=1 b=2
```

Rationale: `+ b=2` only matches when `b=2` is preceded by the `+ ` trace prefix (i.e. on its own trace line). If the implementation combined into `+ a=1 b=2`, the bytes before `b=2` would be `1 ` (no `+`), failing the substring match.

- [ ] **Step 3: Create `set_opt_x_assign_empty.sh`** (empty value)

```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces an empty-value assignment as + name= (trailing equals)
# EXPECT_STDERR: + empty_var=
# EXPECT_EXIT: 0
set -x
empty_var=
```

Variable name `empty_var` chosen so the substring `+ empty_var=` is unique within the trace output (no other assignment shares the prefix).

- [ ] **Step 4: Create `set_opt_x_assign_cmdsub.sh`** (command substitution in value)

```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces assignment with command-sub value after expansion
# EXPECT_STDERR: + x=hi
# EXPECT_EXIT: 0
set -x
x=$(echo hi)
```

The existing command-sub trace `+ echo hi` is already emitted by yosh. The new behaviour is the `+ x=hi` line that follows. Substring `+ x=hi` is uniquely produced by the new code.

- [ ] **Step 5: Create `set_opt_x_assign_ps4.sh`** (PS4 prefix applies)

```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: PS4 prefix is applied to assignment-only trace lines
# EXPECT_STDERR: > x=1
# EXPECT_EXIT: 0
PS4='> '
set -x
x=1
```

Verifies that `xtrace_prefix` (PS4 expansion + replication) is reused for the new trace site.

- [ ] **Step 6: Set 644 permissions on all five files** (per `CLAUDE.md` "E2E test files should have 644 permissions")

```bash
chmod 644 e2e/posix_spec/4_special_builtin/set_opt_x_assign.sh \
          e2e/posix_spec/4_special_builtin/set_opt_x_multi_assign.sh \
          e2e/posix_spec/4_special_builtin/set_opt_x_assign_empty.sh \
          e2e/posix_spec/4_special_builtin/set_opt_x_assign_cmdsub.sh \
          e2e/posix_spec/4_special_builtin/set_opt_x_assign_ps4.sh
```

- [ ] **Step 7: Verify debug build is current** (the e2e runner requires a debug build per `CLAUDE.md`)

```bash
cargo build
```

Expected: clean build (or a "Finished" line if already current).

- [ ] **Step 8: Run the five new tests; expect 5 failures**

```bash
./e2e/run_tests.sh --filter=set_opt_x_assign
```

Expected: 5 tests run, all 5 FAIL with "Stderr: expected substring '…' not found" (because no assignment-only trace is emitted by current yosh).

- [ ] **Step 9: Commit the failing tests**

```bash
git add e2e/posix_spec/4_special_builtin/set_opt_x_assign.sh \
        e2e/posix_spec/4_special_builtin/set_opt_x_multi_assign.sh \
        e2e/posix_spec/4_special_builtin/set_opt_x_assign_empty.sh \
        e2e/posix_spec/4_special_builtin/set_opt_x_assign_cmdsub.sh \
        e2e/posix_spec/4_special_builtin/set_opt_x_assign_ps4.sh
git commit -m "test(e2e): add failing set -x assignment-only trace tests

Five E2E tests pinning the new behaviour (single assignment, multi-assign
per-line, empty value, command-sub value, PS4 prefix). All five fail
against current yosh because the assignment-only branch of
exec_simple_command does not emit a trace.

Spec: docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Implement the trace block

Insert the trace emission into the assignment-only branch of `exec_simple_command`. Position: after `value` is computed (so any nested command-sub trace has already fired), before the `set_with_options` call.

**Files:**
- Modify: `src/exec/simple.rs` (assignment-only branch, between value computation and `set_with_options`)

- [ ] **Step 1: Locate the exact insertion point**

Read `src/exec/simple.rs` lines 180–225 to confirm the current shape of the assignment-only branch. The target site is the `},\n                None => String::new(),\n            };` block (end of value computation) followed by the `// If the value expansion contained a command substitution,` comment.

- [ ] **Step 2: Apply the edit**

Use the Edit tool to replace the closing of the `value` match through the start of the `has_cmd_sub` comment block. Replace this block:

```rust
                    None => String::new(),
                };
                // If the value expansion contained a command substitution, $?
```

with:

```rust
                    None => String::new(),
                };
                // Trace before the variable is set so a readonly-failure still
                // produces the trace line (bash behaviour). Nested command-sub
                // traces inside `value` have already fired during expansion.
                if self.env.mode.options.xtrace {
                    let prefix = xtrace_prefix(&mut self.env);
                    eprintln!("{}{}={}", prefix, assignment.name, value);
                }
                // If the value expansion contained a command substitution, $?
```

The insertion is 5 lines (1 doc comment line plus the 4-line `if` block). No other lines in the file change. `xtrace_prefix` is already in scope (defined at line 63 of the same file).

- [ ] **Step 3: Compile**

```bash
cargo build
```

Expected: clean build, no warnings. (If a warning surfaces about the new block, fix it before proceeding — the project keeps clippy clean.)

- [ ] **Step 4: Run the five tests; expect 5 passes**

```bash
./e2e/run_tests.sh --filter=set_opt_x_assign
```

Expected: 5 tests run, all 5 PASS.

- [ ] **Step 5: Sanity-check that no other e2e test regressed** (run the full `set_opt_x*` family to catch any over-broad trace emission)

```bash
./e2e/run_tests.sh --filter=set_opt_
```

Expected: all `set_opt_*` tests PASS (the existing `set_opt_x_traces.sh` and `set_opt_o_xtrace_alias.sh` plus the five new ones).

- [ ] **Step 6: Commit**

```bash
git add src/exec/simple.rs
git commit -m "feat(exec): trace assignment-only commands under set -x

Emit '+ name=value' (with PS4 prefix) for each assignment in the
assignment-only branch of exec_simple_command. Trace fires after value
expansion (so nested command-sub trace appears first) and before
set_with_options (so a readonly-failure still produces the trace line,
matching bash). Multi-assignment 'a=1 b=2' produces two trace lines.

Closes the only set -x divergence from both bash and dash. Compound
internals and pipeline members were already traced via exec_simple_command
and remain unchanged; for/case structural headers and argument quoting
are out of scope (POSIX implementation-defined; yosh matches dash).

Spec: docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Add a control-flow unit test

Spec §6 asks for at least one unit-level invariant covering the assignment-only branch with `xtrace=true`. This is a **regression guard**, not a TDD-driving test: it passes both before and after the implementation because the trace block does not change control flow. Its purpose is to lock in the invariant.

**Files:**
- Modify: `src/exec/simple.rs` (within the existing `#[cfg(test)] mod tests` block)

- [ ] **Step 1: Locate the existing `mod tests` block**

Open `src/exec/simple.rs` and scroll to the end. The existing tests (e.g. `test_xtrace_prefix_uses_ps4_when_set`) sit inside `#[cfg(test)] mod tests { … }`. Add the new test inside this same block, after the existing `xtrace_prefix` tests.

- [ ] **Step 2: Append the unit test**

Add this test inside `mod tests { … }`:

```rust
#[test]
fn assignment_only_under_xtrace_returns_ok_and_sets_var() {
    use crate::exec::Executor;
    use crate::parser::Parser;

    // Verifies the assignment-only branch's control flow is unchanged
    // by the trace insertion: variable assignment succeeds and the
    // command returns exit status 0 with xtrace enabled.
    let source = "set -x\nfoo=bar";
    let prog = Parser::new(source).parse_program().unwrap();
    let mut exec = Executor::new("yosh", vec![]);
    exec.exec_program(&prog);
    assert_eq!(exec.env.vars.get("foo"), Some("bar"));
    assert_eq!(exec.env.exec.last_exit_status, 0);
}
```

(The `use` lines may already exist at module scope; if they do, drop the inner `use` to avoid `unused_imports`. Check existing tests in the same `mod tests` block — `test_xtrace_prefix_uses_ps4_when_set` already imports `ShellEnv`, so the import pattern is per-test.)

- [ ] **Step 3: Run the new unit test**

```bash
cargo test --lib exec::simple::tests::assignment_only_under_xtrace_returns_ok_and_sets_var
```

Expected: PASS.

- [ ] **Step 4: Run the full `simple` test module to catch any breakage**

```bash
cargo test --lib exec::simple
```

Expected: all tests in `exec::simple` PASS (existing `xtrace_prefix` tests plus the new one).

- [ ] **Step 5: Commit**

```bash
git add src/exec/simple.rs
git commit -m "test(exec): unit guard for assignment-only set -x control flow

Regression test verifying that the trace insertion in the assignment-only
branch does not change control flow: variable assignment still succeeds
and exit status is 0 with xtrace enabled. Stderr-content assertions are
covered at the E2E tier (set_opt_x_assign*.sh).

Spec: docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md §6

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Update TODO.md

Revise the existing "set -x trace coverage is simple-commands-only" entry to reflect the closed assignment-only scope and leave the still-deferred `for`/`case` header + argument quoting items in place. Per `CLAUDE.md`, completed items are **deleted**, not `[x]`-marked. This entry is partially completed; revise to the narrower remaining scope rather than deleting outright.

**Files:**
- Modify: `TODO.md` (currently line ~383, under "Future: Interactive Mode Enhancements")

- [ ] **Step 1: Locate the exact line**

```bash
grep -n "set -x trace coverage" TODO.md
```

Expected: one match, around line 383, with the current text starting `- [ ] \`set -x\` trace coverage is simple-commands-only — xtrace fires only…`.

- [ ] **Step 2: Replace the entry**

Use the Edit tool. Replace this exact block (which spans 5 lines in TODO.md):

```markdown
- [ ] `set -x` trace coverage is simple-commands-only — xtrace fires only in `exec_simple_command`'s non-assignment-only path, so compound commands (`if`/`for`/`while`/`case`), pipelines, and assignment-only commands (e.g. `+ x=1`) are not traced. bash traces all of these. Pre-existing; PS4 full-support spec §6 scoped this out (`docs/superpowers/specs/2026-05-28-set-x-ps4-full-support-design.md`, `src/exec/simple.rs`, `src/exec/compound.rs`).
```

with:

```markdown
- [ ] `set -x` does not emit bash-style structural headers for `for` / `case` (yosh matches dash here; POSIX leaves the header format implementation-defined). Empirical survey 2026-05-28 confirmed compound bodies and pipeline members are already traced via `exec_simple_command`; the assignment-only gap was closed in the 2026-05-28 assignment-trace work. Adding bash parity for the headers requires Word→source rendering plus an xtrace argument-quoting algorithm; the latter also affects existing simple-command trace output (`echo "a b" c` traces as `+ echo a b c` not `+ echo 'a b' c`). Tracked together because both want the same quoting helper. See `docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md` §5 for the closed assignment portion (`src/exec/compound.rs`, `src/exec/simple.rs`).
```

- [ ] **Step 3: Verify the change reads cleanly**

```bash
grep -n "set -x" TODO.md
```

Expected: the new entry appears at the same approximate line; no stray references to the closed assignment-only gap remain.

- [ ] **Step 4: Commit**

```bash
git add TODO.md
git commit -m "docs(todo): close set -x assignment-only gap; retain for/case headers

The 2026-05-28 assignment-trace work closed the assignment-only portion
of the prior set -x coverage entry. Revise the TODO to reflect the
narrower remaining scope: for/case structural headers (bash extension)
plus xtrace argument quoting, both deferred together because they
share an argument-quoting helper requirement.

Spec: docs/superpowers/specs/2026-05-28-set-x-assignment-trace-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Full verification

Confirm nothing else regressed. Per the project memory note, full cargo test runs take minutes; run them anyway as the closing gate.

**Files:** (none modified — verification only)

- [ ] **Step 1: Full unit-test suite**

```bash
cargo test --lib
```

Expected: all tests PASS. (Runtime: a few minutes; this is the user-tolerable upper bound, no need to background unless it exceeds 5 min.)

- [ ] **Step 2: Full integration-test suite** (excludes wasm crates per `CLAUDE.md`)

```bash
cargo test --tests
```

Expected: all tests PASS.

- [ ] **Step 3: Full E2E suite**

```bash
./e2e/run_tests.sh
```

Expected: all tests PASS, including the five new `set_opt_x_assign*.sh` files and unchanged existing PS4 / xtrace tests.

- [ ] **Step 4: Bash-parity sanity check**

```bash
diff <(/bin/bash -c 'set -x; a=1 b=2' 2>&1 1>/dev/null) \
     <(target/debug/yosh -c 'set -x; a=1 b=2' 2>&1 1>/dev/null | grep -v '^yosh: plugin:')
```

Expected: empty diff (yosh's assignment-trace output matches bash byte-for-byte for the default PS4 case). The `grep -v` filter strips the unrelated rich-prompt-plugin "no such file" line that appears on this developer machine; remove the filter if the plugin is installed.

- [ ] **Step 5: No commit** (verification-only task)

---

## Self-Review Notes

**Spec coverage check:**
- §1 problem (assignment-only emits nothing) → Task 1 tests + Task 2 implementation.
- §2 decisions:
  - One line per assignment → Task 1 step 2 (multi-assign test).
  - Expanded value, unquoted → Task 2 step 2 (the `{}={}` format).
  - Trace after expansion, before set → Task 2 step 2 (insertion position).
  - Reuse `xtrace_prefix` per line → Task 2 step 2 (call inside the loop).
- §3 architecture (single insertion point, `xtrace_prefix` reuse) → Task 2.
- §4 error handling (expand-fail = no trace; readonly-fail = trace before error) → guaranteed by insertion position; covered structurally by Task 2.
- §5 out of scope (for/case, quoting, `++`, pipeline/compound bodies) → Task 4 TODO update preserves the deferred items.
- §6 testing:
  - Unit minimum coverage → Task 3.
  - Five E2E files with exact names → Task 1.
- §7 affected files → Tasks 2, 3, 4.
- §8 acceptance:
  - bash vs yosh output equivalence → Task 5 step 4 (diff check).
  - `cargo test` green → Task 5 steps 1, 2.
  - `./e2e/run_tests.sh` green → Task 5 step 3.
  - TODO.md reflects closed scope → Task 4.

**Placeholder scan:** none — every code block and command is concrete.

**Type/symbol consistency:** `xtrace_prefix` (existing fn, `src/exec/simple.rs:63`); `self.env.mode.options.xtrace` (existing bool); `assignment.name` (existing field); `assignment.value` (existing `Option<Word>`); `set_with_options` (unchanged call). All symbols verified against the spec architecture and the current source.
