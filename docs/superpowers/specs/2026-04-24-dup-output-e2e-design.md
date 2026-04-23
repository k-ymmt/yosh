# §2.7.6 DupOutput Dedicated E2E Test Suite

**Date:** 2026-04-24
**Area:** `e2e/posix_spec/2_07_redirection/`
**POSIX reference:** IEEE Std 1003.1 §2.7.6 Duplicating an Output File Descriptor

## Context

POSIX §2.7.5 (Duplicating an Input File Descriptor, `<&`) already has a dedicated 4-test E2E
suite under `e2e/posix_spec/2_07_redirection/dup_input_*.sh`:

- `dup_input_basic.sh`
- `dup_input_param_expansion.sh`
- `dup_input_bad_fd.sh`
- `dup_input_close.sh`

The symmetric §2.7.6 (`>&`) has no equivalent. Current output-dup coverage is limited to
`e2e/redirection/stderr_to_stdout.sh`, which:

- Lives in the legacy (non-`posix_spec`) tree
- Has no `POSIX_REF` metadata
- Only exercises the `2>&1` special case, not `>&N`, `>&-`, bad-fd, or parameter-expanded fd

This is a structural coverage gap. The yosh implementation at `src/exec/redirect.rs:123-141`
handles `DupOutput` with the same shape as `DupInput`; tests should match.

## Goal

Add a dedicated §2.7.6 E2E test suite that mirrors §2.7.5, filling the coverage gap and
enabling per-case regression signal.

## Non-Goals

- Removing or rewriting `e2e/redirection/stderr_to_stdout.sh` (separate cleanup scope).
- Changing the `DupOutput` implementation. Tests pin existing behavior.
- Adding tests for `>&` without a preceding fd (defaulting to 1), covered incidentally by
  `2>&1` in existing tests.

## Design

### Files to add

All under `e2e/posix_spec/2_07_redirection/`, with 644 permissions and the standard
`POSIX_REF` / `DESCRIPTION` / `EXPECT_*` header.

#### 1. `dup_output_basic.sh`

Exercises `exec N>FILE; cmd >&N`, the canonical §2.7.6 shape.

A `file:` prefix via `printf` makes the test fail-closed: if `>&3` is ever
broken to a no-op, `echo hello` leaks to stdout first and the final stdout
(`hello\nfile:`) no longer matches `EXPECT_OUTPUT: file:hello`. Without
this marker, a silently-broken DupOutput would pass because the raw `echo`
output would satisfy `EXPECT_OUTPUT: hello`. DupInput gets this detection
for free because its primary command (`cat <&3`) produces nothing if the
dup is no-ops; for DupOutput we need the marker to close the same gap.

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N duplicates output fd N to fd 1 for the command
# EXPECT_OUTPUT: file:hello
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_out"
exec 3> "$f"
echo hello >&3
exec 3>&-
# 'file:' marker forces fail if >&3 silently became a no-op (see spec)
printf 'file:'
cat "$f"
```

#### 2. `dup_output_param_expansion.sh`

Pins parameter expansion inside the `>&` redirect target, matching
`dup_input_param_expansion.sh`. Uses the same `file:` prefix marker as
`dup_output_basic.sh` to guard against silently-broken-`>&` regressions.

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&"$fd" accepts an fd number via parameter expansion
# EXPECT_OUTPUT: file:hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_out_pe"
exec 3> "$f"
fd=3
echo hi >&"$fd"
exec 3>&-
# 'file:' marker forces fail if >&"$fd" silently became a no-op (see spec)
printf 'file:'
cat "$f"
```

#### 3. `dup_output_bad_fd.sh`

`>&N` against an unopened fd is a redirection error — exit non-zero with `yosh:` prefix on
stderr.

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N for an unopened fd N is a redirection error
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
echo hello >&9
```

#### 4. `dup_output_close.sh`

`>&-` closes an output fd; subsequent `>&N` targeting the closed fd fails.

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&- closes an output fd; subsequent >&N on the same fd fails
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
f="$TEST_TMPDIR/dup_out_close"
exec 3> "$f"
exec 3>&-
echo gone >&3
```

### TODO.md update

Remove the bullet:

> §2.7.6 `>&` (DupOutput) dedicated E2E tests — analogous to the §2.7.5 suite in
> `e2e/posix_spec/2_07_redirection/dup_input_*.sh`. ...

Per project convention (`CLAUDE.md`: "Delete completed items rather than marking them
with `[x]`"), remove the line entirely.

## Verification

1. `chmod 644 e2e/posix_spec/2_07_redirection/dup_output_*.sh` (per CLAUDE.md rule).
2. `./e2e/run_tests.sh --filter=dup_output` — all 4 new tests pass.
3. `./e2e/run_tests.sh` — full E2E suite shows no regression (pre-count = post-count + 4
   for new tests).
4. Smoke `./e2e/run_tests.sh --filter=dup_input` — existing DupInput suite still passes.

## Out-of-Scope Followups

- Legacy `e2e/redirection/stderr_to_stdout.sh` cleanup: once §2.7.6 suite is canonical,
  the legacy test is redundant. A future TODO entry can track its migration or deletion.
- `>&` with defaulted fd (no preceding N): out of scope; covered incidentally by `2>&1`.

## Design Notes (do not simplify)

The `file:` marker in `dup_output_basic.sh` and `dup_output_param_expansion.sh` is
intentional and must not be removed. Without it, a silently-broken `>&N` (e.g., an
implementation regression where the redirect becomes a no-op) would let the raw `echo`
output leak to stdout and incidentally satisfy a plain `EXPECT_OUTPUT: hello`, producing
a false PASS. See the per-file comment and the §1 rationale above. DupInput does not
need this marker because its primary command (`cat <&3`) produces nothing when the dup
is a no-op, so a broken implementation is caught naturally.
