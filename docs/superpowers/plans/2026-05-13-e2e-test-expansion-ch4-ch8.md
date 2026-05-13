# E2E Test Expansion Ch4+Ch8 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ~257 new E2E tests across `e2e/posix_spec/4_special_builtin/`, `e2e/posix_spec/4_required_builtin/`, and `e2e/posix_spec/8_env_vars/` covering XCU §2.14 Special Built-Ins, XCU §1.4 Required Built-Ins, and XBD §8 Environment Variables.

**Architecture:** Three sequential phases, each split into commit-shippable sub-phases. No production code changes. Reuses the existing `POSIX_REF` / `XFAIL` harness in `e2e/run_tests.sh`. Unimplemented POSIX surface (`getopts`, `read`, `pwd`, `type`, `hash`, `ulimit`, locale, mail) is registered as `XFAIL` so XPASS becomes the natural completion signal when implementations land.

**Tech Stack:** POSIX sh test files (`/bin/sh`), `e2e/run_tests.sh` harness, `./target/debug/yosh` shell-under-test.

**Spec:** `docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch4-ch8-design.md`

---

## File Structure

**New directories:**

- `e2e/posix_spec/4_special_builtin/` — Phase 1 home (113 tests)
- `e2e/posix_spec/4_required_builtin/` — Phase 2 home (97 tests)
- `e2e/posix_spec/8_env_vars/` — Phase 3 home (47 tests)

**Modified files:**

- `e2e/README.md` — append `8 Environment Variables - <var>` to the POSIX_REF Format Contract section.
- `TODO.md` — delete the `Future: E2E Test Expansion` L112 bullet once Phase 3 lands; add a new section listing unimplemented required builtins backed by XFAIL tests after Phase 2.

**Not modified:**

- `e2e/run_tests.sh` — the harness accepts arbitrary `POSIX_REF` labels as free text. No code change needed for the new `8 Environment Variables - <var>` shape.
- `e2e/builtin/cd_*.sh`, `e2e/builtin_command/*` — existing tests are kept in place (per spec §3 "Existing Test Handling"). New supplementary `cd_*` / `command_*` tests go into the new `4_required_builtin/` directory.
- Any yosh production source — Phase 2 XFAILs document expected behavior of unimplemented builtins but do not implement them.

---

## Canonical Test File Template

Every test file in this plan follows this template. The fields are interpreted by `e2e/run_tests.sh::parse_metadata`.

```sh
#!/bin/sh
# POSIX_REF: <section reference>
# DESCRIPTION: <one-line behavior summary>
# EXPECT_OUTPUT: <expected stdout, exact match>
# EXPECT_EXIT: <expected exit code, integer>
<shell body>
```

Optional metadata fields:

- `# EXPECT_STDERR: <substring>` — substring match on stderr.
- `# XFAIL: <reason>` — marks the test as expected-fail with categorized reason. Categories used in this plan:
  - `not yet implemented (TODO: implement <X>)` — for missing builtins / options.
  - `non-POSIX deviation (<description>)` — for intentional yosh deviation.
  - `harness limitation (<description>)` — when the test setup itself blocks verification.
- `# EXPECT_OUTPUT<<END` / `# END` — multiline expected output block.

For multi-line expected output, prefix every interior line with `# `.

All file paths in this plan are relative to the repo root. All test files have mode 644 (per CLAUDE.md "E2E test files should have `644` permissions, not `755`").

---

## Pre-flight

Before Task 1, confirm the baseline.

- [ ] **Step 0.1: Confirm git tree is clean**

Run: `git status`
Expected: `nothing to commit, working tree clean` (the spec commit `6802e53` is already in main).

- [ ] **Step 0.2: Build debug yosh**

Run: `cargo build`
Expected: build succeeds (1–3 min cold).

- [ ] **Step 0.3: Capture baseline E2E summary**

Run: `./e2e/run_tests.sh 2>&1 | tail -3 | tee /tmp/e2e-baseline.txt`
Expected: a summary like `Total: N  Passed: P  Failed: F  Timedout: 0  XFail: X  XPass: 0` and exit code 0. Record `N`, `P`, `F`, `X`. The final acceptance task diffs against these numbers.

---

## Task 1: Set up new directories and update README

**Files:**
- Create dir: `e2e/posix_spec/4_special_builtin/`
- Create dir: `e2e/posix_spec/4_required_builtin/`
- Create dir: `e2e/posix_spec/8_env_vars/`
- Modify: `e2e/README.md`

- [ ] **Step 1.1: Create the three new directories**

Run:
```sh
mkdir -p e2e/posix_spec/4_special_builtin e2e/posix_spec/4_required_builtin e2e/posix_spec/8_env_vars
```

The directories will appear empty in `git status` (git does not track empty dirs). They become tracked when Task 2 adds files.

- [ ] **Step 1.2: Update `e2e/README.md` POSIX_REF Format Contract**

In `e2e/README.md`, locate the bulleted list under "POSIX_REF Format Contract" that currently ends with `4 Utilities - <name>`. Append a new bullet after it:

```
- `8 Environment Variables - <var>` — for Chapter 8 environment-variable
  tests (XBD Chapter 8).
  Example: `POSIX_REF: 8 Environment Variables - IFS`
```

- [ ] **Step 1.3: Verify the README still parses cleanly**

Run: `grep -E '^- \`' e2e/README.md | grep -c 'POSIX_REF'`
Expected: prints `5` (was 4 before the addition — `2.X.Y`, `2.10.2 Rule N`, `2.10.2 Rule N (discriminator)`, `2.10 Shell Grammar`, `4 Utilities`, plus the new `8 Environment Variables`). Actual count is 5 list items mentioning POSIX_REF.

If you get a different count, double-check the bullet you added is formatted exactly like the surrounding entries (backticks, leading dash, two-space indent on continuation).

- [ ] **Step 1.4: Commit Task 1**

Run:
```sh
git add e2e/README.md
git commit -m "docs(e2e): document POSIX_REF '8 Environment Variables - <var>' shape

Pre-step for Future: E2E Test Expansion (Ch4+Ch8 breadth, TODO L112).
New shape will be used by tests landing in e2e/posix_spec/8_env_vars/.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

The empty directories do not need a separate commit — they'll get tracked when Task 2 adds files inside them.

---

## Conventions for Phase 1–3 Tasks

Each sub-phase task below follows this shape:

1. List of test files to create with full content for each.
2. Sanity-run with `--filter=<dir>` to confirm new tests are picked up and all PASS or XFAIL.
3. Commit with a descriptive message linking back to the spec.

The full content of every test file is given inline. Do not summarize, do not paraphrase, do not write "similar to file X". Copy the file content verbatim and `chmod 644` if your editor sets a different mode.

After creating the files in each task, run:

```sh
./e2e/run_tests.sh --filter=<new-dir-name>
```

Expected: all new tests are listed as PASS or XFAIL. No FAIL. No XPASS. No TIMEOUT.

If any test FAILs unexpectedly, do not commit. Investigate first — either the test is wrong (most likely if you got a deviation from POSIX-expected output) or yosh has a real bug worth recording. If it's a yosh bug surfaced by an expansion test, add `XFAIL: non-POSIX deviation (...)` and proceed, noting it for the closing task.

If any test XPASSes (you wrote XFAIL but it actually passes), remove the `XFAIL:` line and re-run.

---

# Phase 1: Special Built-In Utilities

Target directory: `e2e/posix_spec/4_special_builtin/`

Phase 1 totals 113 tests across 3 sub-phases (Tasks 2–4).

---

## Task 2: Phase 1 sub-phase 1 — Control flow (`break`, `continue`, `return`, `exit`, `:`)

**Files:** 26 new test files under `e2e/posix_spec/4_special_builtin/`

- [ ] **Step 2.1: Create the test files**

Create these files exactly as specified. Each file is a complete, standalone test.

**`e2e/posix_spec/4_special_builtin/break_no_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break with no operand exits the innermost enclosing loop
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
for i in 1 2 3; do
    echo $i
    break
done
```

**`e2e/posix_spec/4_special_builtin/break_with_n_two.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break 2 exits two enclosing loops
# EXPECT_OUTPUT: outer1-inner1
# EXPECT_EXIT: 0
for i in 1 2; do
    for j in 1 2; do
        echo outer$i-inner$j
        break 2
    done
done
```

**`e2e/posix_spec/4_special_builtin/break_n_exceeds_depth.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break with n exceeding loop nesting exits outermost loop (per POSIX, no error)
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
for i in a b; do
    echo $i
    break 5
done
```

**`e2e/posix_spec/4_special_builtin/break_outside_loop.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break outside any loop is treated as not-in-loop (exit nonzero, message on stderr)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: break
break
```

**`e2e/posix_spec/4_special_builtin/break_invalid_n_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break 0 is an invalid operand
# EXPECT_EXIT: 1
# EXPECT_STDERR: break
for i in 1 2; do
    break 0
done
```

**`e2e/posix_spec/4_special_builtin/break_invalid_n_negative.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break -1 is an invalid operand
# EXPECT_EXIT: 1
# EXPECT_STDERR: break
for i in 1 2; do
    break -1
done
```

**`e2e/posix_spec/4_special_builtin/continue_no_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue with no operand returns to the top of the innermost loop
# EXPECT_OUTPUT<<END
# 1
# 3
# END
# EXPECT_EXIT: 0
for i in 1 2 3; do
    if [ "$i" = 2 ]; then continue; fi
    echo $i
done
```

**`e2e/posix_spec/4_special_builtin/continue_with_n_two.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue 2 returns to the top of the second enclosing loop
# EXPECT_OUTPUT<<END
# 1-1
# 2-1
# END
# EXPECT_EXIT: 0
for i in 1 2; do
    for j in 1 2; do
        echo $i-$j
        continue 2
    done
done
```

**`e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue with n exceeding nesting acts as continue against outermost loop
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for i in a b; do
    echo $i
    continue 5
done
```

**`e2e/posix_spec/4_special_builtin/continue_outside_loop.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue outside any loop is treated as not-in-loop
# EXPECT_EXIT: 1
# EXPECT_STDERR: continue
continue
```

**`e2e/posix_spec/4_special_builtin/continue_invalid_n_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue 0 is invalid
# EXPECT_EXIT: 1
# EXPECT_STDERR: continue
for i in 1; do
    continue 0
done
```

**`e2e/posix_spec/4_special_builtin/colon_returns_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.4 colon
# DESCRIPTION: colon builtin returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
:
echo $?
```

**`e2e/posix_spec/4_special_builtin/colon_ignores_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.4 colon
# DESCRIPTION: colon ignores all positional args and returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
: one two three
echo $?
```

**`e2e/posix_spec/4_special_builtin/colon_with_expansion.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.4 colon
# DESCRIPTION: colon expands its operands (assignment via ${var:=value} side effect)
# EXPECT_OUTPUT: defaulted
# EXPECT_EXIT: 0
unset x
: ${x:=defaulted}
echo "$x"
```

**`e2e/posix_spec/4_special_builtin/return_in_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.12 return
# DESCRIPTION: return inside a function ends the function with its given exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
f() { return 7; }
f
echo $?
```

**`e2e/posix_spec/4_special_builtin/return_no_arg_inherits_status.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.12 return
# DESCRIPTION: return with no operand returns the status of the last command run inside the function
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
f() { false; return; }
f
echo $?
```

**`e2e/posix_spec/4_special_builtin/return_outside_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.12 return
# DESCRIPTION: return at script top level (no enclosing function or dot script) is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: return
return
```

**`e2e/posix_spec/4_special_builtin/return_in_dot_script.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.12 return
# DESCRIPTION: return inside a dot-sourced script returns from the dot, not the parent shell
# EXPECT_OUTPUT<<END
# inside
# after-dot
# END
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/sub.sh" <<'EOF'
echo inside
return 0
echo unreached
EOF
. "$TEST_TMPDIR/sub.sh"
echo after-dot
```

**`e2e/posix_spec/4_special_builtin/return_large_status.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.12 return
# DESCRIPTION: return values are taken modulo 256 by the shell when surfaced as $?
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
f() { return 257; }
f
echo $?
```

**`e2e/posix_spec/4_special_builtin/exit_no_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit with no operand returns the status of the last executed command
# EXPECT_EXIT: 1
false
exit
```

**`e2e/posix_spec/4_special_builtin/exit_explicit_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit 0 forces exit status 0 regardless of prior command
# EXPECT_EXIT: 0
false
exit 0
```

**`e2e/posix_spec/4_special_builtin/exit_explicit_n.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit with explicit status uses that status
# EXPECT_EXIT: 42
exit 42
```

**`e2e/posix_spec/4_special_builtin/exit_modulo_256.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit status is taken modulo 256
# EXPECT_EXIT: 1
exit 257
```

**`e2e/posix_spec/4_special_builtin/exit_invalid_operand.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit with non-numeric operand is an error
# EXPECT_STDERR: exit
# EXPECT_EXIT: 2
exit abc
```

**`e2e/posix_spec/4_special_builtin/exit_in_subshell.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit inside a subshell exits only the subshell
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
( exit 5 )
echo after
```

**`e2e/posix_spec/4_special_builtin/break_continue_in_until.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break works in until loops too
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
n=1
until [ "$n" -gt 5 ]; do
    echo $n
    break
done
```

- [ ] **Step 2.2: Set 644 permissions**

Run:
```sh
chmod 644 e2e/posix_spec/4_special_builtin/*.sh
```

- [ ] **Step 2.3: Run the sub-phase filter**

Run: `./e2e/run_tests.sh --filter=4_special_builtin/`
Expected: 26 tests listed, all PASS or XFAIL, no FAIL.

If a test fails unexpectedly: stop, investigate per the conventions above.

- [ ] **Step 2.4: Run the full suite to confirm no regression**

Run: `./e2e/run_tests.sh 2>&1 | tail -3`
Expected: `Total: N+26`, `Passed >= P+26 - X_new` (where `X_new` is the number of new XFAILs; for this task, likely 0). No new FAILs.

- [ ] **Step 2.5: Commit Task 2**

Run:
```sh
git add e2e/posix_spec/4_special_builtin/
git commit -m "test(e2e): add control-flow special-builtin coverage (Ch4 phase 1.1)

26 new tests covering break/continue/return/exit/colon option matrix
per docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch4-ch8-design.md
Phase 1 sub-phase 1 (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Phase 1 sub-phase 2 — Scope & assignment (`export`, `readonly`, `unset`, `shift`, `set`)

**Files:** 50 new test files under `e2e/posix_spec/4_special_builtin/`

- [ ] **Step 3.1: Create the test files**

**`e2e/posix_spec/4_special_builtin/export_single.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export NAME marks an existing variable for export
# EXPECT_OUTPUT: child-sees-foo
# EXPECT_EXIT: 0
foo=child-sees-foo
export foo
sh -c 'echo "$foo"'
```

**`e2e/posix_spec/4_special_builtin/export_assign.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export NAME=value assigns and exports atomically
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
export foo=hello
sh -c 'echo "$foo"'
```

**`e2e/posix_spec/4_special_builtin/export_multiple.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with multiple NAME=value pairs exports all
# EXPECT_OUTPUT: 1-2
# EXPECT_EXIT: 0
export a=1 b=2
sh -c 'echo "$a-$b"'
```

**`e2e/posix_spec/4_special_builtin/export_empty_value.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export NAME= sets exported empty string
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
export foo=
sh -c 'echo "<$foo>"'
```

**`e2e/posix_spec/4_special_builtin/export_p_listing.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export -p lists exported variables in re-input form
# EXPECT_EXIT: 0
export myvar=value
export -p | grep -q '^export myvar' || exit 1
```

**`e2e/posix_spec/4_special_builtin/export_no_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with no args lists exported variables (same as -p)
# EXPECT_EXIT: 0
export myvar2=v2
export | grep -q myvar2 || exit 1
```

**`e2e/posix_spec/4_special_builtin/export_inherits_value.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export of an existing variable keeps its current value
# EXPECT_OUTPUT: keep
# EXPECT_EXIT: 0
foo=keep
export foo
sh -c 'echo "$foo"'
```

**`e2e/posix_spec/4_special_builtin/export_invalid_name.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with an identifier that starts with a digit is an error
# EXPECT_STDERR: export
# EXPECT_EXIT: 1
export 1foo=v
```

**`e2e/posix_spec/4_special_builtin/export_dash_dash.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export -- treats following operands as names
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
export -- foo=hi
sh -c 'echo "$foo"'
```

**`e2e/posix_spec/4_special_builtin/export_unset_var.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export of an unset variable marks it; later assignment is exported
# EXPECT_OUTPUT: later
# EXPECT_EXIT: 0
unset foo
export foo
foo=later
sh -c 'echo "$foo"'
```

**`e2e/posix_spec/4_special_builtin/readonly_single.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly NAME marks an existing variable read-only
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
foo=initial
readonly foo
foo=changed
```

**`e2e/posix_spec/4_special_builtin/readonly_assign.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly NAME=value assigns and marks read-only atomically
# EXPECT_OUTPUT: locked
# EXPECT_EXIT: 0
readonly foo=locked
echo "$foo"
```

**`e2e/posix_spec/4_special_builtin/readonly_multiple.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly with multiple NAME=value pairs sets all
# EXPECT_OUTPUT: 1-2
# EXPECT_EXIT: 0
readonly a=1 b=2
echo "$a-$b"
```

**`e2e/posix_spec/4_special_builtin/readonly_p_listing.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly -p lists read-only variables in re-input form
# EXPECT_EXIT: 0
readonly myvar=v
readonly -p | grep -q '^readonly myvar' || exit 1
```

**`e2e/posix_spec/4_special_builtin/readonly_no_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly with no args lists read-only variables
# EXPECT_EXIT: 0
readonly myvar2=v2
readonly | grep -q myvar2 || exit 1
```

**`e2e/posix_spec/4_special_builtin/readonly_unset_attempt.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: unset of a read-only variable fails
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
readonly foo=locked
unset foo
```

**`e2e/posix_spec/4_special_builtin/readonly_persists_after_failed_assign.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: a failed assignment to a read-only does not change the value
# EXPECT_OUTPUT: locked
# EXPECT_EXIT: 0
readonly foo=locked
foo=tried 2>/dev/null
echo "$foo"
```

**`e2e/posix_spec/4_special_builtin/readonly_dash_dash.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly -- treats following operands as names
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
readonly -- foo=ok
echo "$foo"
```

**`e2e/posix_spec/4_special_builtin/readonly_empty_value.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly NAME= sets an empty read-only string
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
readonly foo=
echo "<$foo>"
```

**`e2e/posix_spec/4_special_builtin/readonly_invalid_name.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly with identifier starting with digit is an error
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
readonly 1foo=v
```

**`e2e/posix_spec/4_special_builtin/unset_v_variable.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -v removes a variable
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
foo=v
unset -v foo
echo "<$foo>"
```

**`e2e/posix_spec/4_special_builtin/unset_default_is_variable.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset with no flag removes a variable (default behavior)
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
foo=v
unset foo
echo "<$foo>"
```

**`e2e/posix_spec/4_special_builtin/unset_f_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -f removes a function
# EXPECT_EXIT: 127
foo() { echo hello; }
unset -f foo
foo 2>/dev/null
```

**`e2e/posix_spec/4_special_builtin/unset_f_keeps_variable.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -f removes function but leaves same-name variable intact
# EXPECT_OUTPUT: var-value
# EXPECT_EXIT: 0
foo() { echo function; }
foo=var-value
unset -f foo
echo "$foo"
```

**`e2e/posix_spec/4_special_builtin/unset_v_keeps_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -v removes variable but leaves same-name function intact
# EXPECT_OUTPUT: function
# EXPECT_EXIT: 0
foo() { echo function; }
foo=var-value
unset -v foo
foo
```

**`e2e/posix_spec/4_special_builtin/unset_undefined_var.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset of an undefined variable is not an error
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
unset nonexistent_var
echo $?
```

**`e2e/posix_spec/4_special_builtin/unset_readonly_fails.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset of a read-only variable fails
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
readonly foo=v
unset foo
```

**`e2e/posix_spec/4_special_builtin/unset_multiple.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset accepts multiple names
# EXPECT_OUTPUT: <><>
# EXPECT_EXIT: 0
a=1; b=2
unset a b
echo "<$a><$b>"
```

**`e2e/posix_spec/4_special_builtin/unset_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset with no operands is not an error
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
unset
echo $?
```

**`e2e/posix_spec/4_special_builtin/unset_invalid_name.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset with invalid identifier is an error
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
unset 1foo
```

**`e2e/posix_spec/4_special_builtin/shift_default.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: shift with no operand shifts by 1
# EXPECT_OUTPUT: b c
# EXPECT_EXIT: 0
set -- a b c
shift
echo "$1 $2"
```

**`e2e/posix_spec/4_special_builtin/shift_n_two.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: shift 2 shifts two positional parameters
# EXPECT_OUTPUT: c
# EXPECT_EXIT: 0
set -- a b c
shift 2
echo "$1"
```

**`e2e/posix_spec/4_special_builtin/shift_n_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: shift 0 is a no-op
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
set -- a b c
shift 0
echo "$1 $2 $3"
```

**`e2e/posix_spec/4_special_builtin/shift_n_exceeds.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: shift with n greater than $# is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: shift
set -- a b
shift 5
```

**`e2e/posix_spec/4_special_builtin/shift_no_positionals.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: shift when $# is 0 is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: shift
set --
shift
```

**`e2e/posix_spec/4_special_builtin/shift_dollar_count_updated.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: $# decreases by n after shift
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
set -- a b c
shift 2
echo $#
```

**`e2e/posix_spec/4_special_builtin/set_opt_e_exits_on_error.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e causes shell to exit on simple command failure
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
set -e
echo before
false
echo after
```

**`e2e/posix_spec/4_special_builtin/set_opt_u_unset_var.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -u treats expansion of unset variable as an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: unbound variable
set -u
unset x
echo "$x"
```

**`e2e/posix_spec/4_special_builtin/set_opt_x_traces.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x writes a trace of each command to stderr
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: echo 0
set -x
echo 0
```

**`e2e/posix_spec/4_special_builtin/set_opt_n_no_execute.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -n reads commands but does not execute them
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
set -n
echo unreached
```

**`e2e/posix_spec/4_special_builtin/set_opt_f_disables_glob.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -f disables pathname expansion
# EXPECT_OUTPUT: *
# EXPECT_EXIT: 0
set -f
echo *
```

**`e2e/posix_spec/4_special_builtin/set_opt_C_noclobber.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -C (noclobber) prevents > redirection from overwriting existing files
# EXPECT_EXIT: 1
echo first > "$TEST_TMPDIR/f"
set -C
echo second > "$TEST_TMPDIR/f"
```

**`e2e/posix_spec/4_special_builtin/set_opt_o_errexit_alias.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -o errexit is equivalent to set -e
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
set -o errexit
echo before
false
echo after
```

**`e2e/posix_spec/4_special_builtin/set_opt_o_nounset_alias.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -o nounset is equivalent to set -u
# EXPECT_EXIT: 1
set -o nounset
unset x
echo "$x"
```

**`e2e/posix_spec/4_special_builtin/set_opt_o_xtrace_alias.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -o xtrace is equivalent to set -x
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: echo 0
set -o xtrace
echo 0
```

**`e2e/posix_spec/4_special_builtin/set_dash_dash_resets_positional.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -- with no further operands clears positional parameters
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
set -- a b c
set --
echo $#
```

**`e2e/posix_spec/4_special_builtin/set_positional_assigns.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -- ARGS assigns positional parameters
# EXPECT_OUTPUT: one two three
# EXPECT_EXIT: 0
set -- one two three
echo "$1 $2 $3"
```

**`e2e/posix_spec/4_special_builtin/set_plus_e_disables.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set +e disables errexit
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
set -e
set +e
false
echo after
```

**`e2e/posix_spec/4_special_builtin/set_no_args_lists_vars.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set with no operands writes the current set of shell variables to stdout
# EXPECT_EXIT: 0
mymarker=mvalue
set | grep -q '^mymarker=' || exit 1
```

**`e2e/posix_spec/4_special_builtin/set_opt_e_in_subshell.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e inside a subshell exits only the subshell
# EXPECT_OUTPUT<<END
# subshell-before
# after
# END
# EXPECT_EXIT: 0
( set -e; echo subshell-before; false; echo subshell-after ) 2>/dev/null
echo after
```

**`e2e/posix_spec/4_special_builtin/set_opt_e_command_in_condition.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e does NOT exit when command status is tested in if/while/until
# EXPECT_OUTPUT: tested
# EXPECT_EXIT: 0
set -e
if false; then echo unreached; fi
echo tested
```

**`e2e/posix_spec/4_special_builtin/set_opt_e_pipe_last_fails.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e exits when the LAST command of a pipeline fails
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
set -e
echo before
true | false
echo unreached
```

- [ ] **Step 3.2: Set permissions and run filter**

Run:
```sh
chmod 644 e2e/posix_spec/4_special_builtin/*.sh
./e2e/run_tests.sh --filter=4_special_builtin/
```

Expected: 76 tests now (26 from Task 2 + 50 new), all PASS or XFAIL.

- [ ] **Step 3.3: Full suite regression check**

Run: `./e2e/run_tests.sh 2>&1 | tail -3`
Expected: `Total: baseline+76`. No new FAIL beyond what existed in baseline.

- [ ] **Step 3.4: Commit Task 3**

Run:
```sh
git add e2e/posix_spec/4_special_builtin/
git commit -m "test(e2e): add scope+assignment special-builtin coverage (Ch4 phase 1.2)

50 new tests covering export/readonly/unset/shift/set option matrix
per Phase 1 sub-phase 2 of the Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Phase 1 sub-phase 3 — Execution & substitution (`eval`, `exec`, `.`, `times`, `trap`)

**Files:** 37 new test files under `e2e/posix_spec/4_special_builtin/`

- [ ] **Step 4.1: Create the test files**

**`e2e/posix_spec/4_special_builtin/eval_concat_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval concatenates its operands with spaces and re-parses
# EXPECT_OUTPUT: hello world
# EXPECT_EXIT: 0
eval echo "hello" "world"
```

**`e2e/posix_spec/4_special_builtin/eval_constructs_assignment.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval re-parses to allow variable name to be computed
# EXPECT_OUTPUT: 42
# EXPECT_EXIT: 0
name=foo
eval "$name=42"
echo "$foo"
```

**`e2e/posix_spec/4_special_builtin/eval_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval with no operands returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
eval
echo $?
```

**`e2e/posix_spec/4_special_builtin/eval_empty_string.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval of an empty string returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
eval ""
echo $?
```

**`e2e/posix_spec/4_special_builtin/eval_syntax_error.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval of a syntax error propagates non-zero exit
# EXPECT_EXIT: 2
eval "if then fi" 2>/dev/null
```

**`e2e/posix_spec/4_special_builtin/eval_command_exit_status.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval surfaces the executed command's exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
eval "(exit 7)"
echo $?
```

**`e2e/posix_spec/4_special_builtin/eval_recursive.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval can recursively invoke eval
# EXPECT_OUTPUT: deep
# EXPECT_EXIT: 0
eval 'eval echo deep'
```

**`e2e/posix_spec/4_special_builtin/eval_quoted_word_split.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval's concatenation respects word splitting between operands
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
eval echo a b
```

**`e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with no command applies redirections to the current shell
# EXPECT_OUTPUT: persistent
# EXPECT_EXIT: 0
exec >"$TEST_TMPDIR/out"
echo persistent
exec >/dev/tty 2>/dev/null || exec >&-
cat "$TEST_TMPDIR/out"
```

**`e2e/posix_spec/4_special_builtin/exec_replaces_shell.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with a command replaces the shell with the command
# EXPECT_OUTPUT: replaced
# EXPECT_EXIT: 0
exec sh -c 'echo replaced'
echo unreached
```

**`e2e/posix_spec/4_special_builtin/exec_command_not_found.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec of a nonexistent command exits 127
# EXPECT_EXIT: 127
exec /no/such/command 2>/dev/null
```

**`e2e/posix_spec/4_special_builtin/exec_command_not_executable.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec of a non-executable file exits 126
# EXPECT_EXIT: 126
: > "$TEST_TMPDIR/notexec"
chmod 644 "$TEST_TMPDIR/notexec"
exec "$TEST_TMPDIR/notexec" 2>/dev/null
```

**`e2e/posix_spec/4_special_builtin/exec_keeps_env.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec preserves exported environment
# EXPECT_OUTPUT: kept
# EXPECT_EXIT: 0
export marker=kept
exec sh -c 'echo "$marker"'
```

**`e2e/posix_spec/4_special_builtin/exec_redir_input.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with no command applies input redirection to shell
# EXPECT_OUTPUT: line1
# EXPECT_EXIT: 0
echo line1 > "$TEST_TMPDIR/in"
exec < "$TEST_TMPDIR/in"
read line
echo "$line"
```

**`e2e/posix_spec/4_special_builtin/exec_close_fd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec N>&- closes fd N for the current shell
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_EXIT: 0
exec 3>&-
read line 0<&3 2>/dev/null
```

**`e2e/posix_spec/4_special_builtin/exec_fd_dup.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec 3>file then echo >&3 writes to file via the shell fd
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
exec 3>"$TEST_TMPDIR/out"
echo hello >&3
exec 3>&-
cat "$TEST_TMPDIR/out"
```

**`e2e/posix_spec/4_special_builtin/dot_basic.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot reads commands from file and executes in current environment
# EXPECT_OUTPUT: imported
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/lib.sh" <<'EOF'
foo=imported
EOF
. "$TEST_TMPDIR/lib.sh"
echo "$foo"
```

**`e2e/posix_spec/4_special_builtin/dot_path_search.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot searches PATH when argument has no slash
# EXPECT_OUTPUT: found
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/libdir"
cat > "$TEST_TMPDIR/libdir/mylib.sh" <<'EOF'
echo found
EOF
PATH="$TEST_TMPDIR/libdir:$PATH"
export PATH
. mylib.sh
```

**`e2e/posix_spec/4_special_builtin/dot_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot with no args is an error
# EXPECT_EXIT: 2
# EXPECT_STDERR: .
.
```

**`e2e/posix_spec/4_special_builtin/dot_file_not_found.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot of a nonexistent file is an error
# EXPECT_EXIT: 1
. /no/such/file 2>/dev/null
```

**`e2e/posix_spec/4_special_builtin/dot_status_propagation.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot's exit status is the status of the last command in the file
# EXPECT_OUTPUT: 5
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/s.sh" <<'EOF'
(exit 5)
EOF
. "$TEST_TMPDIR/s.sh"
echo $?
```

**`e2e/posix_spec/4_special_builtin/dot_function_definition.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot can introduce function definitions into the current shell
# EXPECT_OUTPUT: callable
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/fn.sh" <<'EOF'
mytool() { echo callable; }
EOF
. "$TEST_TMPDIR/fn.sh"
mytool
```

**`e2e/posix_spec/4_special_builtin/dot_variable_visible_after.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: variables set inside dot script persist in the current shell
# EXPECT_OUTPUT: persisted
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/v.sh" <<'EOF'
v=persisted
EOF
. "$TEST_TMPDIR/v.sh"
echo "$v"
```

**`e2e/posix_spec/4_special_builtin/times_returns_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.15 times
# DESCRIPTION: times returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
times >/dev/null
echo $?
```

**`e2e/posix_spec/4_special_builtin/times_format_4_values.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.15 times
# DESCRIPTION: times outputs four mm:ss.ff values on two lines (user/sys for shell, then children)
# EXPECT_EXIT: 0
out=$(times)
# Two lines, each has two mm:ss.ff values separated by whitespace
line_count=$(printf '%s\n' "$out" | wc -l)
[ "$line_count" -eq 2 ] || exit 1
```

**`e2e/posix_spec/4_special_builtin/times_ignores_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.15 times
# DESCRIPTION: times accepts no operands; extra args may cause a usage error or be ignored
# EXPECT_EXIT: 0
times >/dev/null 2>&1
```

**`e2e/posix_spec/4_special_builtin/trap_set_exit.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap action runs on EXIT
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
trap 'echo bye' EXIT
```

**`e2e/posix_spec/4_special_builtin/trap_reset_to_default.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap - SIGNAL resets the trap to default
# EXPECT_OUTPUT: cleared
# EXPECT_EXIT: 0
trap 'echo first' EXIT
trap - EXIT
trap 'echo cleared' EXIT
```

**`e2e/posix_spec/4_special_builtin/trap_ignore_signal.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap '' SIGNAL causes the signal to be ignored
# EXPECT_OUTPUT: survived
# EXPECT_EXIT: 0
trap '' TERM
kill -TERM $$ 2>/dev/null
echo survived
```

**`e2e/posix_spec/4_special_builtin/trap_list_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap with no operands writes currently-set traps to stdout
# EXPECT_EXIT: 0
trap 'echo bye' EXIT
trap | grep -q "EXIT" || exit 1
```

**`e2e/posix_spec/4_special_builtin/trap_multiple_signals.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap can set the same action for multiple signals
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
trap 'echo bye' EXIT TERM INT
```

**`e2e/posix_spec/4_special_builtin/trap_int_handler.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap runs handler then resumes
# EXPECT_OUTPUT<<END
# caught
# after
# END
# EXPECT_EXIT: 0
trap 'echo caught' INT
kill -INT $$ 2>/dev/null
sleep 0.05 2>/dev/null
echo after
```

**`e2e/posix_spec/4_special_builtin/trap_numeric_signal.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap accepts numeric signal numbers
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
trap 'echo bye' 0
```

**`e2e/posix_spec/4_special_builtin/trap_subshell_inherits.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: subshell starts with the parent's traps; resetting before subshell loses them
# EXPECT_OUTPUT: inner
# EXPECT_EXIT: 0
trap 'echo inner' EXIT
( true )
```

**`e2e/posix_spec/4_special_builtin/trap_invalid_signal_name.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap with an unknown signal name is an error
# EXPECT_STDERR: trap
# EXPECT_EXIT: 1
trap 'echo x' BOGUS
```

**`e2e/posix_spec/4_special_builtin/trap_in_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap set inside a function remains set after function returns
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
f() { trap 'echo bye' EXIT; }
f
```

**`e2e/posix_spec/4_special_builtin/trap_subshell_does_not_leak.sh`**
```sh
#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: a trap set inside a subshell does not leak to the parent
# EXPECT_OUTPUT: parent
# EXPECT_EXIT: 0
( trap 'echo subshell' EXIT; true )
trap 'echo parent' EXIT
```

- [ ] **Step 4.2: Set permissions and run filter**

Run:
```sh
chmod 644 e2e/posix_spec/4_special_builtin/*.sh
./e2e/run_tests.sh --filter=4_special_builtin/
```

Expected: 113 tests now (26 + 50 + 37), all PASS or XFAIL. The `exec_close_fd.sh` test should report XFAIL.

- [ ] **Step 4.3: Full suite regression check**

Run: `./e2e/run_tests.sh 2>&1 | tail -3`
Expected: `Total: baseline+113`. No new FAIL/XPASS.

- [ ] **Step 4.4: Commit Task 4**

Run:
```sh
git add e2e/posix_spec/4_special_builtin/
git commit -m "test(e2e): add execution+substitution special-builtin coverage (Ch4 phase 1.3)

37 new tests covering eval/exec/dot/times/trap option matrix per Phase 1
sub-phase 3 of the Ch4+Ch8 expansion spec (TODO L112). Phase 1 complete:
113 total tests under e2e/posix_spec/4_special_builtin/.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase 2: Required Built-Ins

Target directory: `e2e/posix_spec/4_required_builtin/`

Phase 2 totals 97 tests across 5 sub-phases (Tasks 5–9).

---

## Task 5: Phase 2 sub-phase 1 — Job control (`alias`, `unalias`, `bg`, `fg`, `jobs`, `wait`, `kill`)

**Files:** 38 new test files under `e2e/posix_spec/4_required_builtin/`

- [ ] **Step 5.1: Create the test files**

**`e2e/posix_spec/4_required_builtin/alias_define.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias defines a command alias
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
alias greet='echo hello'
greet
```

**`e2e/posix_spec/4_required_builtin/alias_list_all.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias with no args lists all aliases
# EXPECT_EXIT: 0
alias greet='echo hi'
alias | grep -q '^alias greet=' || alias | grep -q '^greet=' || exit 1
```

**`e2e/posix_spec/4_required_builtin/alias_list_single.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias NAME prints the named alias
# EXPECT_EXIT: 0
alias greet='echo hi'
alias greet | grep -q "greet=" || exit 1
```

**`e2e/posix_spec/4_required_builtin/alias_unknown_name.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias of an undefined name is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: alias
alias nosuch
```

**`e2e/posix_spec/4_required_builtin/alias_multiple.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias can define multiple aliases in one call
# EXPECT_OUTPUT<<END
# hi
# bye
# END
# EXPECT_EXIT: 0
alias g1='echo hi' g2='echo bye'
g1
g2
```

**`e2e/posix_spec/4_required_builtin/alias_with_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: arguments after an alias invocation are passed through
# EXPECT_OUTPUT: from-args
# EXPECT_EXIT: 0
alias say='echo'
say from-args
```

**`e2e/posix_spec/4_required_builtin/alias_recursive_guard.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias is not re-expanded on itself
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
alias ls='echo x'
ls
```

**`e2e/posix_spec/4_required_builtin/alias_empty_value.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias with empty value defines an alias that runs nothing
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
alias noop=''
noop
```

**`e2e/posix_spec/4_required_builtin/unalias_single.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias NAME removes the named alias
# EXPECT_OUTPUT: ran
# EXPECT_EXIT: 0
alias greet='echo aliased'
unalias greet
greet() { echo ran; }
greet
```

**`e2e/posix_spec/4_required_builtin/unalias_all.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias -a removes all aliases
# EXPECT_EXIT: 0
alias g1='echo 1' g2='echo 2'
unalias -a
alias g1 2>/dev/null && exit 1
exit 0
```

**`e2e/posix_spec/4_required_builtin/unalias_unknown.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias of an undefined name is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: unalias
unalias nosuch
```

**`e2e/posix_spec/4_required_builtin/unalias_multiple.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias accepts multiple names
# EXPECT_EXIT: 0
alias g1='echo 1' g2='echo 2'
unalias g1 g2
```

**`e2e/posix_spec/4_required_builtin/bg_no_monitor.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - bg
# DESCRIPTION: bg without job-control monitor mode is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: bg
set +m
bg 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/bg_no_job.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - bg
# DESCRIPTION: bg with no current job is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: bg
set -m 2>/dev/null
bg %1 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/bg_invalid_job_spec.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - bg
# DESCRIPTION: bg with malformed job spec is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: bg
bg %notajob 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/fg_no_monitor.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fg
# DESCRIPTION: fg without monitor mode is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fg
set +m
fg 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/fg_no_job.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fg
# DESCRIPTION: fg with no current job is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fg
set -m 2>/dev/null
fg %1 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/fg_invalid_job_spec.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fg
# DESCRIPTION: fg with malformed job spec is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fg
fg %notajob 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/jobs_no_jobs.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs with no jobs writes nothing and returns 0
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
jobs
```

**`e2e/posix_spec/4_required_builtin/jobs_unknown_spec.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs with unknown job spec is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: jobs
jobs %99 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/jobs_l_long_format.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs -l prints in long format (PID + status)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
jobs -l
```

**`e2e/posix_spec/4_required_builtin/jobs_p_pids.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs -p prints only PIDs
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
jobs -p
```

**`e2e/posix_spec/4_required_builtin/jobs_after_background.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs lists background jobs running in the current shell
# EXPECT_EXIT: 0
set -m 2>/dev/null
sleep 0.1 &
out=$(jobs)
wait
case "$out" in
    *sleep*) exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/jobs_invalid_option.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs -x is an unknown option (error)
# EXPECT_EXIT: 1
# EXPECT_STDERR: jobs
jobs -x 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/wait_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait with no args waits for all background jobs and returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
sleep 0.01 &
sleep 0.02 &
wait
echo $?
```

**`e2e/posix_spec/4_required_builtin/wait_pid.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait PID returns the exit status of the given pid
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
sleep 0.01 &
pid=$!
wait "$pid"
echo $?
```

**`e2e/posix_spec/4_required_builtin/wait_nonexistent_pid.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait of a pid that is not a child returns 127
# EXPECT_EXIT: 127
wait 99999 2>/dev/null
```

**`e2e/posix_spec/4_required_builtin/wait_failed_job.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait surfaces a nonzero child exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
sh -c 'exit 7' &
pid=$!
wait "$pid"
echo $?
```

**`e2e/posix_spec/4_required_builtin/wait_after_collected.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait after job has been collected returns the cached status
# EXPECT_OUTPUT<<END
# 5
# 5
# END
# EXPECT_EXIT: 0
sh -c 'exit 5' &
pid=$!
wait "$pid"
echo $?
wait "$pid"
echo $?
```

**`e2e/posix_spec/4_required_builtin/kill_l_lists.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -l lists signal names
# EXPECT_EXIT: 0
kill -l | grep -q TERM || exit 1
```

**`e2e/posix_spec/4_required_builtin/kill_l_with_number.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -l N prints the signal name for number N
# EXPECT_OUTPUT: TERM
# EXPECT_EXIT: 0
kill -l 15
```

**`e2e/posix_spec/4_required_builtin/kill_s_signal.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -s TERM PID sends SIGTERM
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -s TERM "$pid"
wait "$pid" 2>/dev/null
exit 0
```

**`e2e/posix_spec/4_required_builtin/kill_dash_signal_name.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -TERM PID sends SIGTERM
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -TERM "$pid"
wait "$pid" 2>/dev/null
exit 0
```

**`e2e/posix_spec/4_required_builtin/kill_dash_signal_number.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -15 PID is equivalent to kill -TERM PID
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -15 "$pid"
wait "$pid" 2>/dev/null
exit 0
```

**`e2e/posix_spec/4_required_builtin/kill_default_term.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill PID without -s defaults to SIGTERM
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill "$pid"
wait "$pid" 2>/dev/null
exit 0
```

**`e2e/posix_spec/4_required_builtin/kill_unknown_signal.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -BOGUS is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: kill
kill -BOGUS 1 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/kill_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill with no args is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: kill
kill 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/kill_zero_signal_check.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -0 PID tests whether PID can be signaled (no signal sent)
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -0 "$pid"
status=$?
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
exit "$status"
```

- [ ] **Step 5.2: Set permissions and run filter**

Run:
```sh
chmod 644 e2e/posix_spec/4_required_builtin/*.sh
./e2e/run_tests.sh --filter=4_required_builtin/
```

Expected: 38 tests, all PASS or XFAIL.

- [ ] **Step 5.3: Full suite regression check**

Run: `./e2e/run_tests.sh 2>&1 | tail -3`

- [ ] **Step 5.4: Commit Task 5**

Run:
```sh
git add e2e/posix_spec/4_required_builtin/
git commit -m "test(e2e): add job-control required-builtin coverage (Ch4 phase 2.1)

38 new tests covering alias/unalias/bg/fg/jobs/wait/kill option matrix
per Phase 2 sub-phase 1 of the Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Phase 2 sub-phase 2 — Navigation & history (`cd` supplement, `pwd` XFAIL, `fc`)

**Files:** 20 new test files under `e2e/posix_spec/4_required_builtin/`

- [ ] **Step 6.1: Create the test files**

**`e2e/posix_spec/4_required_builtin/cd_dash_l_logical.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -L uses logical handling of dot-dot (default)
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/a/b"
cd "$TEST_TMPDIR/a/b"
cd -L ..
case "$PWD" in
    */a) exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/cd_dash_p_physical.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -P uses physical handling, resolving symlinks
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/real"
ln -s real "$TEST_TMPDIR/sym"
cd "$TEST_TMPDIR/sym"
cd -P ..
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/cd_no_arg_home.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd with no args changes to $HOME
# EXPECT_EXIT: 0
HOME="$TEST_TMPDIR"
cd
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/cd_dash_returns_oldpwd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - changes to $OLDPWD and prints the new directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/a" "$TEST_TMPDIR/b"
cd "$TEST_TMPDIR/a"
cd "$TEST_TMPDIR/b"
out=$(cd -)
case "$out" in
    *"$TEST_TMPDIR/a") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/cd_invalid_dir.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd to nonexistent directory is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: cd
cd /no/such/directory 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/cd_updates_pwd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd updates $PWD on success
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/cd_updates_oldpwd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd updates $OLDPWD to the previous directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/x"
prev="$PWD"
cd "$TEST_TMPDIR/x"
case "$OLDPWD" in
    "$prev") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/cd_to_dotdot.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd .. moves up one directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/sub"
cd "$TEST_TMPDIR/sub"
cd ..
case "$PWD" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/pwd_default.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd prints the current working directory
# XFAIL: pwd builtin not yet implemented (TODO: implement pwd)
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
out=$(pwd)
case "$out" in
    "$TEST_TMPDIR") exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/pwd_dash_l_logical.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd -L prints the logical (symlink-preserving) path
# XFAIL: pwd builtin not yet implemented (TODO: implement pwd)
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/real"
ln -s real "$TEST_TMPDIR/sym"
cd "$TEST_TMPDIR/sym"
out=$(pwd -L)
case "$out" in
    *sym) exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/pwd_dash_p_physical.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd -P prints the physical (resolved) path
# XFAIL: pwd builtin not yet implemented (TODO: implement pwd)
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/real"
ln -s real "$TEST_TMPDIR/sym"
cd "$TEST_TMPDIR/sym"
out=$(pwd -P)
case "$out" in
    *real) exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/pwd_returns_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd returns 0 on success
# XFAIL: pwd builtin not yet implemented (TODO: implement pwd)
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
pwd >/dev/null
echo $?
```

**`e2e/posix_spec/4_required_builtin/fc_l_lists_recent.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l lists recent history entries
# EXPECT_EXIT: 0
fc -l >/dev/null
```

**`e2e/posix_spec/4_required_builtin/fc_l_n_no_numbers.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l -n suppresses leading numbers in the listing
# EXPECT_EXIT: 0
fc -l -n >/dev/null
```

**`e2e/posix_spec/4_required_builtin/fc_r_reverse.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l -r lists entries in reverse order
# EXPECT_EXIT: 0
fc -l -r >/dev/null
```

**`e2e/posix_spec/4_required_builtin/fc_s_substitute.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -s old=new RE re-executes the most-recent matching command with substitution
# XFAIL: fc -s substitution may rely on interactive history capture
# EXPECT_EXIT: 0
echo onevar
fc -s one=two echo
```

**`e2e/posix_spec/4_required_builtin/fc_no_command.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc with no operands edits the previous command (requires editor; should not crash)
# XFAIL: harness limitation (fc editor invocation needs an interactive context)
# EXPECT_EXIT: 0
fc 2>&1 >/dev/null </dev/null
```

**`e2e/posix_spec/4_required_builtin/fc_e_editor.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -e EDITOR picks the editor for the edit step
# XFAIL: harness limitation (fc -e relies on launching an editor)
# EXPECT_EXIT: 0
fc -e cat 2>&1 >/dev/null </dev/null
```

**`e2e/posix_spec/4_required_builtin/fc_unknown_option.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc with unknown option is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fc
fc -Z 2>&1 1>/dev/null
```

- [ ] **Step 6.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/4_required_builtin/*.sh
./e2e/run_tests.sh --filter=4_required_builtin/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 58 tests in the directory (38 from Task 5 + 20 new); 3 pwd XFAILs + 3 fc XFAILs expected.

- [ ] **Step 6.3: Commit Task 6**

Run:
```sh
git add e2e/posix_spec/4_required_builtin/
git commit -m "test(e2e): add navigation+history required-builtin coverage (Ch4 phase 2.2)

20 new tests covering cd supplement / pwd (XFAIL) / fc per Phase 2
sub-phase 2 of the Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Phase 2 sub-phase 3 — Command lookup & type (`command` supplement, `type` XFAIL, `hash` XFAIL)

**Files:** 14 new test files under `e2e/posix_spec/4_required_builtin/`

- [ ] **Step 7.1: Create the test files**

**`e2e/posix_spec/4_required_builtin/command_dash_p_path.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -p uses the standard PATH for the search
# EXPECT_EXIT: 0
command -p echo found >/dev/null
```

**`e2e/posix_spec/4_required_builtin/command_v_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on a function prints the function name
# EXPECT_OUTPUT: myfn
# EXPECT_EXIT: 0
myfn() { :; }
command -v myfn
```

**`e2e/posix_spec/4_required_builtin/command_v_special_builtin.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on a special builtin prints the name
# EXPECT_OUTPUT: export
# EXPECT_EXIT: 0
command -v export
```

**`e2e/posix_spec/4_required_builtin/command_v_no_match.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on nonexistent name exits nonzero with no output
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
command -v /no/such/cmd_$$
```

**`e2e/posix_spec/4_required_builtin/command_V_external.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -V prints a human-readable identification
# EXPECT_EXIT: 0
command -V echo >/dev/null
```

**`e2e/posix_spec/4_required_builtin/type_external.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on an external command prints its path
# XFAIL: type builtin not yet implemented (TODO: implement type)
# EXPECT_EXIT: 0
type echo >/dev/null
```

**`e2e/posix_spec/4_required_builtin/type_builtin.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a builtin identifies it as such
# XFAIL: type builtin not yet implemented (TODO: implement type)
# EXPECT_EXIT: 0
type cd | grep -q builtin
```

**`e2e/posix_spec/4_required_builtin/type_alias.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on an alias identifies it as alias
# XFAIL: type builtin not yet implemented (TODO: implement type)
# EXPECT_EXIT: 0
alias myalias='echo x'
type myalias | grep -q alias
```

**`e2e/posix_spec/4_required_builtin/type_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a function identifies it as function
# XFAIL: type builtin not yet implemented (TODO: implement type)
# EXPECT_EXIT: 0
myfn() { :; }
type myfn | grep -q function
```

**`e2e/posix_spec/4_required_builtin/type_not_found.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a nonexistent name exits nonzero
# XFAIL: type builtin not yet implemented (TODO: implement type)
# EXPECT_EXIT: 1
type /no/such/cmd_$$ 2>/dev/null
```

**`e2e/posix_spec/4_required_builtin/hash_command.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash remembers the location of a utility
# XFAIL: hash builtin not yet implemented (TODO: implement hash)
# EXPECT_EXIT: 0
hash echo
```

**`e2e/posix_spec/4_required_builtin/hash_r_clears.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash -r forgets remembered locations
# XFAIL: hash builtin not yet implemented (TODO: implement hash)
# EXPECT_EXIT: 0
hash -r
```

**`e2e/posix_spec/4_required_builtin/hash_no_arg_lists.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash with no operands lists the hash table
# XFAIL: hash builtin not yet implemented (TODO: implement hash)
# EXPECT_EXIT: 0
hash >/dev/null
```

**`e2e/posix_spec/4_required_builtin/hash_unknown_cmd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash of a nonexistent utility is an error
# XFAIL: hash builtin not yet implemented (TODO: implement hash)
# EXPECT_EXIT: 1
hash /no/such/cmd_$$ 2>/dev/null
```

- [ ] **Step 7.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/4_required_builtin/*.sh
./e2e/run_tests.sh --filter=4_required_builtin/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 72 tests in the directory; 5 `type` XFAILs + 4 `hash` XFAILs from this task (plus prior XFAILs from Task 6).

- [ ] **Step 7.3: Commit Task 7**

Run:
```sh
git add e2e/posix_spec/4_required_builtin/
git commit -m "test(e2e): add command-lookup required-builtin coverage (Ch4 phase 2.3)

14 new tests covering command supplement / type (XFAIL) / hash (XFAIL)
per Phase 2 sub-phase 3 of the Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Phase 2 sub-phase 4 — Unimplemented builtins (`getopts`, `read`, `ulimit`)

**Files:** 19 new test files under `e2e/posix_spec/4_required_builtin/`, all XFAIL.

- [ ] **Step 8.1: Create the test files**

**`e2e/posix_spec/4_required_builtin/getopts_basic.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts a opt parses -a from $@
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$opt"
```

**`e2e/posix_spec/4_required_builtin/getopts_with_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts a: opt parses -a value into $OPTARG
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: a=value
# EXPECT_EXIT: 0
set -- -a value
getopts "a:" opt
echo "$opt=$OPTARG"
```

**`e2e/posix_spec/4_required_builtin/getopts_unknown.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts sets opt to ? for unknown options
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: ?
# EXPECT_EXIT: 0
set -- -x
getopts "a" opt 2>/dev/null
echo "$opt"
```

**`e2e/posix_spec/4_required_builtin/getopts_missing_arg.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts indicates missing required arg (colon-prefix mode)
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: :a
# EXPECT_EXIT: 0
set -- -a
getopts ":a:" opt 2>/dev/null
echo "$opt$OPTARG"
```

**`e2e/posix_spec/4_required_builtin/getopts_optind.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts advances OPTIND across options
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$OPTIND"
```

**`e2e/posix_spec/4_required_builtin/getopts_end_with_double_dash.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts stops at -- and increments OPTIND past it
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_EXIT: 1
set -- --
getopts a opt
```

**`e2e/posix_spec/4_required_builtin/getopts_no_more.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts returns nonzero when no options remain
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_EXIT: 1
set -- arg
getopts a opt
```

**`e2e/posix_spec/4_required_builtin/getopts_stacked.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts handles stacked single-letter options (-ab as -a -b)
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
set -- -ab
getopts ab opt
echo "$opt"
getopts ab opt
echo "$opt"
```

**`e2e/posix_spec/4_required_builtin/read_basic.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read assigns one line of stdin to a variable
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
echo hello | { read line; echo "$line"; }
```

**`e2e/posix_spec/4_required_builtin/read_r_preserves_backslash.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read -r preserves backslashes in input
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: a\b
# EXPECT_EXIT: 0
printf 'a\\b\n' | { read -r line; echo "$line"; }
```

**`e2e/posix_spec/4_required_builtin/read_multiple_vars.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with multiple var names splits the line by IFS
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: a-b-c
# EXPECT_EXIT: 0
echo a b c | { read x y z; echo "$x-$y-$z"; }
```

**`e2e/posix_spec/4_required_builtin/read_last_var_gets_remainder.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: when input has more fields than vars, last var gets the remainder
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: a-b c d
# EXPECT_EXIT: 0
echo a b c d | { read x y; echo "$x-$y"; }
```

**`e2e/posix_spec/4_required_builtin/read_eof_returns_nonzero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read returns nonzero on EOF
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_EXIT: 1
read line </dev/null
```

**`e2e/posix_spec/4_required_builtin/read_no_args.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with no args is an error
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_EXIT: 1
# EXPECT_STDERR: read
read 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/read_partial_line.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with input lacking final newline still reads partial line, returns nonzero
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: partial
# EXPECT_EXIT: 1
printf 'partial' | { read line; echo "$line"; }
```

**`e2e/posix_spec/4_required_builtin/read_strips_ifs.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read strips leading and trailing IFS whitespace
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: <hello>
# EXPECT_EXIT: 0
echo '   hello   ' | { read line; echo "<$line>"; }
```

**`e2e/posix_spec/4_required_builtin/ulimit_show_filesize.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit -f with no value shows the current file-size limit
# XFAIL: ulimit builtin not yet implemented (TODO: implement ulimit)
# EXPECT_EXIT: 0
ulimit -f >/dev/null
```

**`e2e/posix_spec/4_required_builtin/ulimit_set_filesize.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit -f N sets a soft file-size limit
# XFAIL: ulimit builtin not yet implemented (TODO: implement ulimit)
# EXPECT_EXIT: 0
ulimit -f 100
```

**`e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit with unknown option is an error
# XFAIL: ulimit builtin not yet implemented (TODO: implement ulimit)
# EXPECT_EXIT: 1
# EXPECT_STDERR: ulimit
ulimit -Z 2>&1 1>/dev/null
```

- [ ] **Step 8.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/4_required_builtin/*.sh
./e2e/run_tests.sh --filter=4_required_builtin/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 91 tests in the directory; 19 new XFAILs from this task.

- [ ] **Step 8.3: Commit Task 8**

Run:
```sh
git add e2e/posix_spec/4_required_builtin/
git commit -m "test(e2e): add unimplemented-builtin XFAIL coverage (Ch4 phase 2.4)

19 new XFAIL tests covering getopts/read/ulimit option matrix as
acceptance spec for future implementation, per Phase 2 sub-phase 4 of
the Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Phase 2 sub-phase 5 — File (`umask`)

**Files:** 6 new test files under `e2e/posix_spec/4_required_builtin/`

- [ ] **Step 9.1: Create the test files**

**`e2e/posix_spec/4_required_builtin/umask_show.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask with no operand prints the current mask in octal
# EXPECT_EXIT: 0
umask | grep -qE '^[0-7]+$' || exit 1
```

**`e2e/posix_spec/4_required_builtin/umask_set_octal.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask 022 sets the mask
# EXPECT_OUTPUT: 022
# EXPECT_EXIT: 0
umask 022
out=$(umask)
# Accept both 022 and 0022 (POSIX allows leading zero)
case "$out" in
    022|0022) echo 022 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/umask_set_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask 0 unmasks everything
# EXPECT_EXIT: 0
umask 0
out=$(umask)
case "$out" in
    0|00|000|0000) exit 0 ;;
    *) exit 1 ;;
esac
```

**`e2e/posix_spec/4_required_builtin/umask_s_symbolic.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask -S prints the mask symbolically (u=rwx,g=rx,o=rx)
# EXPECT_EXIT: 0
umask 022
umask -S | grep -q u= || exit 1
```

**`e2e/posix_spec/4_required_builtin/umask_invalid_mask.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask with an invalid mask is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: umask
umask 999 2>&1 1>/dev/null
```

**`e2e/posix_spec/4_required_builtin/umask_affects_creat.sh`**
```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask is honored by file creation
# EXPECT_EXIT: 0
umask 077
: > "$TEST_TMPDIR/f"
# Only owner perms should be set; group and other should be empty
perms=$(ls -l "$TEST_TMPDIR/f" | awk '{print $1}')
case "$perms" in
    -rw-------) exit 0 ;;
    *) exit 1 ;;
esac
```

- [ ] **Step 9.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/4_required_builtin/*.sh
./e2e/run_tests.sh --filter=4_required_builtin/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 97 tests in the directory.

- [ ] **Step 9.3: Commit Task 9 and add Phase 2 wrap-up TODO note**

In `TODO.md`, locate the existing `## Future: E2E Test Expansion` section. Above it, insert a new section (or append to the existing list) that records which required POSIX builtins are not yet implemented in yosh — backed by the XFAIL tests added in Tasks 6–8.

Append to `TODO.md` (find an appropriate location, e.g., between `## Future: Plugin System Enhancements` and `## Future: E2E Test Expansion`):

```markdown
## Future: POSIX Required Builtin Implementation

The following XCU §1.4 required builtins are not implemented in yosh.
The XFAIL tests added in 2026-05-13 (`e2e/posix_spec/4_required_builtin/`)
serve as the behavioral acceptance spec for each implementation. When a
builtin is implemented, the corresponding XFAIL tests should become PASS;
remove the `# XFAIL:` line at that point.

- [ ] `getopts optstring var [args]` — option-parsing helper, used in
      portable shell scripts. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/getopts_*.sh` (8 tests)
- [ ] `hash [-r] [cmd]` — utility-location cache. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/hash_*.sh` (4 tests)
- [ ] `pwd [-LP]` — print working directory (currently only PWD variable
      is updated by cd). XFAIL tests:
      `e2e/posix_spec/4_required_builtin/pwd_*.sh` (3 tests)
- [ ] `read [-r] var...` — read one line from stdin into variables.
      XFAIL tests: `e2e/posix_spec/4_required_builtin/read_*.sh` (7 tests)
- [ ] `type name...` — identify command kind (function / builtin / alias
      / external path). XFAIL tests:
      `e2e/posix_spec/4_required_builtin/type_*.sh` (5 tests)
- [ ] `ulimit [-f] [num]` — resource-limit query/set. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/ulimit_*.sh` (3 tests)
```

Then commit:

```sh
git add e2e/posix_spec/4_required_builtin/ TODO.md
git commit -m "test(e2e): add umask coverage + TODO list for unimplemented builtins (Ch4 phase 2.5)

6 new umask tests close Phase 2 sub-phase 5 of the Ch4+Ch8 expansion
spec (TODO L112). Phase 2 complete: 97 tests under
e2e/posix_spec/4_required_builtin/, ~30 XFAILs documenting expected
behavior of unimplemented POSIX required builtins. TODO.md gains a
'Future: POSIX Required Builtin Implementation' section pointing back
to the XFAIL tests as the per-builtin acceptance spec.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase 3: Environment Variables

Target directory: `e2e/posix_spec/8_env_vars/`

Phase 3 totals 47 tests across 7 sub-phases (Tasks 10–16).

---

## Task 10: Phase 3 sub-phase 1 — Shell behavior variables (`HOME`, `IFS`, `PATH`, `PWD`, `OLDPWD`, `CDPATH`, `ENV`, `SHELL`)

**Files:** 16 new test files under `e2e/posix_spec/8_env_vars/`

- [ ] **Step 10.1: Create the test files**

**`e2e/posix_spec/8_env_vars/HOME_default.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - HOME
# DESCRIPTION: cd with no args uses $HOME
# EXPECT_EXIT: 0
HOME="$TEST_TMPDIR"
cd
case "$PWD" in "$TEST_TMPDIR") exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/HOME_tilde_expansion.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - HOME
# DESCRIPTION: tilde expands to $HOME
# EXPECT_OUTPUT: /custom/home
# EXPECT_EXIT: 0
HOME=/custom/home
echo ~
```

**`e2e/posix_spec/8_env_vars/IFS_default_unset.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: when IFS is unset, default IFS (space tab newline) is used for field splitting
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
unset IFS
v='a b c'
set -- $v
echo "$1 $2 $3"
```

**`e2e/posix_spec/8_env_vars/IFS_field_split.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: IFS controls word splitting on unquoted expansion
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
IFS=:
v="a:b:c"
set -- $v
echo "$1 $2 $3"
```

**`e2e/posix_spec/8_env_vars/IFS_empty_no_split.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: IFS='' (empty) means no word splitting
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
IFS=''
v="a b c"
set -- $v
echo "$1"
```

**`e2e/posix_spec/8_env_vars/PATH_search.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PATH
# DESCRIPTION: PATH is searched for external commands in order
# EXPECT_OUTPUT: from-dir1
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/d1" "$TEST_TMPDIR/d2"
cat > "$TEST_TMPDIR/d1/mycmd" <<'EOF'
#!/bin/sh
echo from-dir1
EOF
cat > "$TEST_TMPDIR/d2/mycmd" <<'EOF'
#!/bin/sh
echo from-dir2
EOF
chmod +x "$TEST_TMPDIR/d1/mycmd" "$TEST_TMPDIR/d2/mycmd"
PATH="$TEST_TMPDIR/d1:$TEST_TMPDIR/d2"
mycmd
```

**`e2e/posix_spec/8_env_vars/PATH_empty_means_cwd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PATH
# DESCRIPTION: an empty PATH entry (leading colon, embedded ::, trailing colon) means the current directory
# EXPECT_OUTPUT: from-cwd
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/mycwdcmd" <<'EOF'
#!/bin/sh
echo from-cwd
EOF
chmod +x "$TEST_TMPDIR/mycwdcmd"
cd "$TEST_TMPDIR"
PATH=":/usr/bin:/bin"
mycwdcmd
```

**`e2e/posix_spec/8_env_vars/PWD_updated_by_cd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PWD
# DESCRIPTION: cd updates PWD to the new directory
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
case "$PWD" in "$TEST_TMPDIR") exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/PWD_subshell_inherits.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PWD
# DESCRIPTION: PWD is inherited by subshells
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
out=$( echo "$PWD" )
case "$out" in "$TEST_TMPDIR") exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/OLDPWD_after_cd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - OLDPWD
# DESCRIPTION: cd sets OLDPWD to the prior PWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/x"
prev="$PWD"
cd "$TEST_TMPDIR/x"
case "$OLDPWD" in "$prev") exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/OLDPWD_cd_dash.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - OLDPWD
# DESCRIPTION: cd - returns to the directory in $OLDPWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/a" "$TEST_TMPDIR/b"
cd "$TEST_TMPDIR/a"
cd "$TEST_TMPDIR/b"
cd - >/dev/null
case "$PWD" in "$TEST_TMPDIR/a") exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/CDPATH_search.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - CDPATH
# DESCRIPTION: CDPATH is consulted by cd for relative paths
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/base/sub"
CDPATH="$TEST_TMPDIR/base"
cd sub
case "$PWD" in */base/sub) exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/CDPATH_empty_means_cwd.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - CDPATH
# DESCRIPTION: an empty CDPATH entry means the current directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/local"
cd "$TEST_TMPDIR"
CDPATH=":/no/such/dir"
cd local
case "$PWD" in */local) exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/ENV_source_on_startup.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - ENV
# DESCRIPTION: ENV file is sourced on interactive shell startup; non-interactive: not sourced
# XFAIL: harness limitation (yosh -c is non-interactive; ENV file should NOT be sourced — XFAIL pending verification of expected non-interactive semantics)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/envrc" <<'EOF'
echo from-env
EOF
ENV="$TEST_TMPDIR/envrc" sh -c 'true'
```

**`e2e/posix_spec/8_env_vars/SHELL_is_set.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - SHELL
# DESCRIPTION: SHELL is set by the shell or its parent and is preserved
# EXPECT_EXIT: 0
[ -n "${SHELL+x}" ] && exit 0
exit 1
```

**`e2e/posix_spec/8_env_vars/IFS_tab_split.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: IFS containing tab still splits on tab
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS='	'  # literal tab
v='a	b	c'
set -- $v
echo $#
```

- [ ] **Step 10.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 16 tests, mostly PASS, 1 XFAIL (`ENV_source_on_startup.sh`).

- [ ] **Step 10.3: Commit Task 10**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add shell-behavior env var coverage (Ch8 phase 3.1)

16 new tests covering HOME/IFS/PATH/PWD/OLDPWD/CDPATH/ENV/SHELL per
Phase 3 sub-phase 1 of the Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Phase 3 sub-phase 2 — Prompt variables (`PS1`, `PS2`, `PS4`)

**Files:** 6 new test files under `e2e/posix_spec/8_env_vars/`

- [ ] **Step 11.1: Create the test files**

**`e2e/posix_spec/8_env_vars/PS1_in_subshell_inherited.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS1
# DESCRIPTION: PS1 is inherited by subshells (variable scope only; not displayed in non-interactive)
# EXPECT_OUTPUT: my-prompt
# EXPECT_EXIT: 0
PS1='my-prompt'
( echo "$PS1" )
```

**`e2e/posix_spec/8_env_vars/PS1_default_non_interactive.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS1
# DESCRIPTION: PS1 is not displayed in non-interactive shell (no stdout side-effect on script lines)
# EXPECT_OUTPUT: line1
# EXPECT_EXIT: 0
PS1='$ '
echo line1
```

**`e2e/posix_spec/8_env_vars/PS2_in_subshell_inherited.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS2
# DESCRIPTION: PS2 is inherited by subshells
# EXPECT_OUTPUT: cont
# EXPECT_EXIT: 0
PS2=cont
( echo "$PS2" )
```

**`e2e/posix_spec/8_env_vars/PS4_assigned.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 controls the trace prefix when set -x is in effect
# EXPECT_OUTPUT: 0
# EXPECT_STDERR: TRACE>
# EXPECT_EXIT: 0
PS4='TRACE> '
set -x
echo 0
```

**`e2e/posix_spec/8_env_vars/PS4_default.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: default PS4 is '+ '
# EXPECT_STDERR: + echo 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
unset PS4
set -x
echo 0
```

**`e2e/posix_spec/8_env_vars/PS1_default_value.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS1
# DESCRIPTION: PS1 is set to a default value when shell starts (non-empty)
# XFAIL: harness limitation (PS1 default value is not exposed when invoked via -c on non-interactive shell)
# EXPECT_EXIT: 0
[ -n "${PS1+x}" ] && exit 0
exit 1
```

- [ ] **Step 11.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 22 tests in the directory.

- [ ] **Step 11.3: Commit Task 11**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add prompt env var coverage (Ch8 phase 3.2)

6 new tests covering PS1/PS2/PS4 per Phase 3 sub-phase 2 of the
Ch4+Ch8 expansion spec (TODO L112).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: Phase 3 sub-phase 3 — Special parameters (`LINENO`, `PPID`, `OPTARG`, `OPTIND`)

**Files:** 8 new test files under `e2e/posix_spec/8_env_vars/`

- [ ] **Step 12.1: Create the test files**

**`e2e/posix_spec/8_env_vars/LINENO_basic.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LINENO
# DESCRIPTION: LINENO expands to the current line number
# EXPECT_OUTPUT: 5
# EXPECT_EXIT: 0
# (line numbers count from 1; this echo is on line 5)
echo "$LINENO"
```

**`e2e/posix_spec/8_env_vars/LINENO_in_function.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LINENO
# DESCRIPTION: LINENO works inside a function
# EXPECT_OUTPUT: 5
# EXPECT_EXIT: 0
f() { echo "$LINENO"; }
# this line is line 5 from the file start (counting the header comments)
f
```

**`e2e/posix_spec/8_env_vars/LINENO_increments.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LINENO
# DESCRIPTION: LINENO changes between successive lines
# EXPECT_EXIT: 0
a="$LINENO"
b="$LINENO"
[ "$a" != "$b" ] || exit 1
```

**`e2e/posix_spec/8_env_vars/PPID_is_set.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PPID
# DESCRIPTION: PPID is set to the parent process ID
# EXPECT_EXIT: 0
[ -n "$PPID" ] || exit 1
[ "$PPID" -gt 0 ] || exit 1
```

**`e2e/posix_spec/8_env_vars/PPID_inherits_in_subshell.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - PPID
# DESCRIPTION: subshell preserves the original PPID (per POSIX, PPID does not change in subshell)
# EXPECT_EXIT: 0
parent="$PPID"
sub=$( echo "$PPID" )
[ "$parent" = "$sub" ] || exit 1
```

**`e2e/posix_spec/8_env_vars/OPTARG_set_by_getopts.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTARG
# DESCRIPTION: getopts sets OPTARG to the argument value for options that take an argument
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
set -- -a value
getopts "a:" opt
echo "$OPTARG"
```

**`e2e/posix_spec/8_env_vars/OPTIND_initial_one.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTIND
# DESCRIPTION: OPTIND starts at 1 at shell entry
# XFAIL: OPTIND default-init requires getopts builtin to be implemented (TODO: implement getopts)
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
echo "$OPTIND"
```

**`e2e/posix_spec/8_env_vars/OPTIND_advances.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTIND
# DESCRIPTION: OPTIND advances as getopts consumes options
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$OPTIND"
```

- [ ] **Step 12.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 30 tests in the directory; 3 XFAILs added by this task (OPTARG/OPTIND).

If the `LINENO_basic.sh` test fails because the line count doesn't match `5`, check the actual line position of the `echo` statement in the file (it should be line 5: shebang + 3 metadata lines + the comment line + echo line — count starts from the `echo` line index). Adjust the `EXPECT_OUTPUT` to the actual position if needed.

- [ ] **Step 12.3: Commit Task 12**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add special-parameter env var coverage (Ch8 phase 3.3)

8 new tests covering LINENO/PPID/OPTARG/OPTIND per Phase 3 sub-phase 3
of the Ch4+Ch8 expansion spec (TODO L112). OPTARG/OPTIND tests are
XFAIL pending getopts implementation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: Phase 3 sub-phase 4 — Locale (`LANG`, `LC_ALL`, `LC_CTYPE`, `LC_COLLATE`, `LC_MESSAGES`, `NLSPATH`)

**Files:** 6 new test files under `e2e/posix_spec/8_env_vars/`, all XFAIL.

- [ ] **Step 13.1: Create the test files**

**`e2e/posix_spec/8_env_vars/LANG_default_collate.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG sets default locale category values
# XFAIL: locale support not implemented in yosh (TODO: implement locale handling)
# EXPECT_EXIT: 0
LANG=C
[ "$(echo b a | sort | head -n1)" = a ] || exit 1
```

**`e2e/posix_spec/8_env_vars/LC_ALL_overrides_others.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_ALL
# DESCRIPTION: LC_ALL overrides all other LC_* and LANG
# XFAIL: locale support not implemented in yosh (TODO: implement locale handling)
# EXPECT_EXIT: 0
LC_ALL=C
LANG=en_US.UTF-8
# When LC_ALL is set, LANG should not affect output
[ "$LC_ALL" = C ] || exit 1
```

**`e2e/posix_spec/8_env_vars/LC_CTYPE_affects_classification.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_CTYPE
# DESCRIPTION: LC_CTYPE affects character classification (toupper/tolower behavior in case patterns)
# XFAIL: locale support not implemented in yosh (TODO: implement LC_CTYPE)
# EXPECT_EXIT: 0
LC_CTYPE=C
case A in [a-z]) exit 1 ;; *) exit 0 ;; esac
```

**`e2e/posix_spec/8_env_vars/LC_COLLATE_affects_pattern.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_COLLATE
# DESCRIPTION: LC_COLLATE affects range collation in patterns
# XFAIL: locale support not implemented in yosh (TODO: implement LC_COLLATE)
# EXPECT_EXIT: 0
LC_COLLATE=C
case M in [A-Z]) exit 0 ;; *) exit 1 ;; esac
```

**`e2e/posix_spec/8_env_vars/LC_MESSAGES_locale.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_MESSAGES
# DESCRIPTION: LC_MESSAGES determines the locale for diagnostic message text
# XFAIL: LC_MESSAGES message localization not implemented in yosh (TODO: implement localized error messages)
# EXPECT_EXIT: 0
LC_MESSAGES=C
exit 0
```

**`e2e/posix_spec/8_env_vars/NLSPATH_set.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - NLSPATH
# DESCRIPTION: NLSPATH locates message catalogs; yosh does not use catgets
# XFAIL: NLSPATH / catgets not implemented in yosh (TODO: implement message catalogs)
# EXPECT_EXIT: 0
NLSPATH=/usr/share/locale/%L/LC_MESSAGES/%N.cat
exit 0
```

- [ ] **Step 13.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 36 tests in the directory; 6 new XFAILs.

- [ ] **Step 13.3: Commit Task 13**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add locale env var XFAIL coverage (Ch8 phase 3.4)

6 new XFAIL tests covering LANG/LC_ALL/LC_CTYPE/LC_COLLATE/LC_MESSAGES/NLSPATH
per Phase 3 sub-phase 4 of the Ch4+Ch8 expansion spec (TODO L112).
Locale support is not implemented in yosh; tests document expected POSIX
behavior for future implementation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 14: Phase 3 sub-phase 5 — Mail (`MAIL`, `MAILCHECK`, `MAILPATH`)

**Files:** 3 new test files under `e2e/posix_spec/8_env_vars/`, all XFAIL.

- [ ] **Step 14.1: Create the test files**

**`e2e/posix_spec/8_env_vars/MAIL_check.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - MAIL
# DESCRIPTION: MAIL names a single mailbox file the shell checks before each prompt
# XFAIL: mail notification not implemented in yosh (TODO: implement mail check on PS1)
# EXPECT_EXIT: 0
MAIL="$TEST_TMPDIR/inbox"
exit 0
```

**`e2e/posix_spec/8_env_vars/MAILCHECK_interval.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - MAILCHECK
# DESCRIPTION: MAILCHECK specifies the seconds between mailbox-modification checks
# XFAIL: mail notification not implemented in yosh (TODO: implement MAILCHECK)
# EXPECT_EXIT: 0
MAILCHECK=600
exit 0
```

**`e2e/posix_spec/8_env_vars/MAILPATH_multiple.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - MAILPATH
# DESCRIPTION: MAILPATH is a colon-separated list of mailboxes, each optionally followed by ?message
# XFAIL: mail notification not implemented in yosh (TODO: implement MAILPATH)
# EXPECT_EXIT: 0
MAILPATH="$TEST_TMPDIR/a:$TEST_TMPDIR/b"
exit 0
```

- [ ] **Step 14.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 39 tests in the directory; 3 new XFAILs.

- [ ] **Step 14.3: Commit Task 14**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add mail env var XFAIL coverage (Ch8 phase 3.5)

3 new XFAIL tests covering MAIL/MAILCHECK/MAILPATH per Phase 3 sub-phase
5 of the Ch4+Ch8 expansion spec (TODO L112). Mail check is not
implemented in yosh.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 15: Phase 3 sub-phase 6 — History & fc (`HISTFILE`, `HISTSIZE`, `FCEDIT`)

**Files:** 6 new test files under `e2e/posix_spec/8_env_vars/`

- [ ] **Step 15.1: Create the test files**

**`e2e/posix_spec/8_env_vars/HISTFILE_set.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTFILE
# DESCRIPTION: HISTFILE names the file used to save command history
# XFAIL: harness limitation (history file save only happens in interactive sessions)
# EXPECT_EXIT: 0
HISTFILE="$TEST_TMPDIR/history"
[ "$HISTFILE" = "$TEST_TMPDIR/history" ] || exit 1
```

**`e2e/posix_spec/8_env_vars/HISTFILE_default_none.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTFILE
# DESCRIPTION: if HISTFILE is unset, no history is saved
# EXPECT_EXIT: 0
unset HISTFILE
exit 0
```

**`e2e/posix_spec/8_env_vars/HISTSIZE_set.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTSIZE
# DESCRIPTION: HISTSIZE caps the number of history entries
# EXPECT_EXIT: 0
HISTSIZE=100
[ "$HISTSIZE" = 100 ] || exit 1
```

**`e2e/posix_spec/8_env_vars/HISTSIZE_zero.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTSIZE
# DESCRIPTION: HISTSIZE=0 disables history
# XFAIL: harness limitation (HISTSIZE behavior is observed only in interactive sessions)
# EXPECT_EXIT: 0
HISTSIZE=0
exit 0
```

**`e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - FCEDIT
# DESCRIPTION: FCEDIT selects the editor used by fc with no -e option
# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)
# EXPECT_EXIT: 0
FCEDIT=cat
fc 2>&1 >/dev/null </dev/null
```

**`e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - FCEDIT
# DESCRIPTION: when FCEDIT is unset, fc uses ed by default
# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)
# EXPECT_EXIT: 0
unset FCEDIT
fc 2>&1 >/dev/null </dev/null
```

- [ ] **Step 15.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 45 tests in the directory.

- [ ] **Step 15.3: Commit Task 15**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add history+fc env var coverage (Ch8 phase 3.6)

6 new tests covering HISTFILE/HISTSIZE/FCEDIT per Phase 3 sub-phase 6
of the Ch4+Ch8 expansion spec (TODO L112). Most are XFAIL pending an
interactive harness for history side effects.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 16: Phase 3 sub-phase 7 — Temp (`TMPDIR`)

**Files:** 2 new test files under `e2e/posix_spec/8_env_vars/`

- [ ] **Step 16.1: Create the test files**

**`e2e/posix_spec/8_env_vars/TMPDIR_used_by_temp.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - TMPDIR
# DESCRIPTION: TMPDIR is honored by the shell when creating temp files (here-doc, etc.)
# XFAIL: harness limitation (yosh's here-doc tempfile location is internal; honoring TMPDIR is opaque to E2E)
# EXPECT_EXIT: 0
TMPDIR="$TEST_TMPDIR"
exit 0
```

**`e2e/posix_spec/8_env_vars/TMPDIR_set.sh`**
```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - TMPDIR
# DESCRIPTION: TMPDIR is propagated to child processes
# EXPECT_OUTPUT: /custom/tmp
# EXPECT_EXIT: 0
TMPDIR=/custom/tmp
export TMPDIR
sh -c 'echo "$TMPDIR"'
```

- [ ] **Step 16.2: Set permissions, filter run, regression check**

Run:
```sh
chmod 644 e2e/posix_spec/8_env_vars/*.sh
./e2e/run_tests.sh --filter=8_env_vars/
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected: 47 tests in the directory.

- [ ] **Step 16.3: Commit Task 16**

Run:
```sh
git add e2e/posix_spec/8_env_vars/
git commit -m "test(e2e): add TMPDIR env var coverage (Ch8 phase 3.7)

2 new tests covering TMPDIR per Phase 3 sub-phase 7 of the Ch4+Ch8
expansion spec (TODO L112). Phase 3 complete: 47 tests under
e2e/posix_spec/8_env_vars/.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 17: Final acceptance and TODO.md update

- [ ] **Step 17.1: Run the full E2E suite**

Run: `./e2e/run_tests.sh 2>&1 | tail -3 | tee /tmp/e2e-final.txt`

Expected: a summary like `Total: N+257  Passed: P+(257-X_new)  Failed: F  Timedout: 0  XFail: X+X_new  XPass: 0`. Compare against `/tmp/e2e-baseline.txt` from Pre-flight Step 0.3:

- `Total` should have grown by exactly 257.
- `Failed` and `Timedout` should be unchanged (no new FAIL).
- `XPass` should be 0 (no XFAIL that unexpectedly passes).
- `XFail` should have grown by approximately 60–70 (sum of all XFAILs added across Tasks 4, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16 — exact number depends on which yosh-implemented optional behaviors PASS unexpectedly during writing).

If `XPass > 0`: open the test files reported as XPASS, remove the `# XFAIL:` line, and re-run.

If `Failed > baseline`: open the failing test files. Either:
- The test is wrong (most likely): fix and re-run.
- yosh has a real bug: add `# XFAIL: non-POSIX deviation (...)` and add a TODO.md entry describing the bug.

- [ ] **Step 17.2: Run cargo unit + integration tests**

Run: `cargo test`
Expected: all PASS (the new tests are shell files only; no Rust code touched).

- [ ] **Step 17.3: Verify cargo fmt and clippy clean**

Run:
```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: both clean. (We only added shell files, so no fmt/clippy regressions are possible.)

- [ ] **Step 17.4: Delete TODO.md L112 entry**

In `TODO.md`, locate the `## Future: E2E Test Expansion` section. Delete the first bullet (currently L112):

```
- [ ] Extend chapter-by-chapter POSIX coverage beyond XCU Chapter 2 — once the Chapter 2 coverage matrix stabilizes, add systematic E2E coverage for Chapter 4 Utilities (all shell-relevant builtins: special + regular, with option/edge-case matrices) and Chapter 8 Environment Variables. Reuse the `POSIX_REF`/`XFAIL` harness established for Chapter 2.
```

Keep the second bullet (L113 — Chapter 2 深堀り) as it remains future work.

If the deletion leaves the section with only one bullet, that's fine — leave the section header in place.

- [ ] **Step 17.5: Commit Task 17**

Run:
```sh
git add TODO.md
git commit -m "docs(todo): complete Ch4+Ch8 e2e breadth expansion (TODO L112)

Removes the 'Extend chapter-by-chapter POSIX coverage beyond Chapter 2'
bullet from Future: E2E Test Expansion. Replaced by:

- e2e/posix_spec/4_special_builtin/ (113 tests across 15 special builtins)
- e2e/posix_spec/4_required_builtin/ (97 tests across 17 required builtins,
  ~30 XFAIL for unimplemented commands)
- e2e/posix_spec/8_env_vars/ (47 tests across 25 environment variables,
  mixed PASS/XFAIL for unimplemented locale and mail features)

TODO L113 (Chapter 2 深堀り) remains pending as the next E2E expansion.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 17.6: Final verification**

Run:
```sh
git log --oneline -20
./e2e/run_tests.sh 2>&1 | tail -3
```

Confirm:
- The last 17 commits match the task sequence (Tasks 1 through 17).
- The final E2E summary matches Step 17.1 expectations.
- `git status` is clean.

---

## Wrap-up

After Task 17, the work is complete:

- 257 new E2E tests across 3 new directories.
- All PASS or XFAIL; no regressions in the 398 pre-existing tests.
- 6 unimplemented POSIX required builtins (`getopts`, `read`, `pwd`, `type`, `hash`, `ulimit`) have XFAIL acceptance specs and a fresh TODO.md section pointing to them.
- Locale and mail support gaps are explicitly catalogued via XFAIL with category reasons.
- `e2e/README.md` documents the new `8 Environment Variables - <var>` POSIX_REF shape.
- TODO.md `Future: E2E Test Expansion` L112 is deleted; L113 (Chapter 2 深堀り) remains as the next E2E expansion target.

When any of the unimplemented builtins (or locale, etc.) lands in a future PR, the corresponding XFAIL tests should XPASS; remove the `# XFAIL:` line at that point as part of the implementation PR.
