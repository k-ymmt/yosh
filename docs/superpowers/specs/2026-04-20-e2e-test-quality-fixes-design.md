# E2E Test Quality Fixes

## Goal

Tighten 5 existing E2E tests from TODO.md §"Future: E2E Test Expansion" that have quality issues (false-pass risk, weak assertions, duplicates, wrong permissions). Scope is limited to **existing** tests; new tests and coverage gaps are out of scope for this spec.

## Scope

The following 5 items from TODO.md:

1. `e2e/command_execution/echo_simple.sh` has `755` permissions — should be `644`.
2. `rule05_for_valid_name_ok.sh` duplicates `rule05_for_valid_name.sh`.
3. `rule04_case_last_item_no_dsemi.sh` — both arms emit the same output; assertion is too weak.
4. `times_format.sh` / `times_in_subshell.sh` — glob `*m*s\ *m*s` matches nonsense like `msms ms ms`.
5. `rule10_reserved_quoted_not_recognized.sh` — false-passes on any exit ≠ 2, including exit 0 when an `if` executable exists on PATH.

## Changes

### 1. `echo_simple.sh` permissions

`chmod 644 e2e/command_execution/echo_simple.sh` to match project convention (E2E scripts are read by the runner, not executed directly).

### 2. Remove `rule05_for_valid_name_ok.sh`

Both files guard POSIX §2.10.2 Rule 5 valid-NAME acceptance under the same `POSIX_REF`. Keep `rule05_for_valid_name.sh` (older, referenced in prior sub-project work); delete `_ok.sh`.

### 3. Restructure `rule04_case_last_item_no_dsemi.sh`

Change body to:

```sh
case x in
    a) echo FIRST ;;
    x) echo LAST
esac
```

`EXPECT_OUTPUT: LAST`. This yields three distinct observable failure modes:

- `a` arm wrongly matches `x` → `FIRST` → output mismatch
- Parser rejects trailing no-`;;` arm → syntax error (exit 2)
- Correct behavior → `LAST`

### 4. Tighten times globs

In both `times_format.sh` and `times_in_subshell.sh`, replace:

```sh
*m*s\ *m*s)
```

with:

```sh
[0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s)
```

**Implementation note:** Yosh's `builtin_times` in `src/builtin/special.rs` formats each time as `{}m{:.3}s`, so the decimal point is always present (confirmed).

### 5. Strengthen `rule10_reserved_quoted_not_recognized.sh`

Replace the "any exit ≠ 2" check with an explicit "exit == 127" assertion under a guaranteed-missing PATH:

```sh
PATH=/nonexistent/path 'if' true 2>/dev/null
rc=$?
if [ "$rc" -eq 2 ]; then
    echo "syntax error detected (rc=2); quoted 'if' was treated as reserved word" >&2
    exit 1
fi
if [ "$rc" -ne 127 ]; then
    echo "expected 127 (command not found), got $rc" >&2
    exit 1
fi
echo ok
```

**Note on TODO suggestion:** The TODO proposed `PATH=/nonexistent env 'if' true`, but routing through `env` adds ambiguity (the shell may fail to find `env` itself in `/nonexistent`, producing indistinguishable failure from a reserved-word rejection). Setting PATH directly on the quoted `if` invocation is cleaner: the shell's own command-lookup path is exercised, and exit 127 is the only non-false-pass outcome.

The explanatory comment block at the top stays; the only change is the assertion body.

## Verification

For each change:

1. `./e2e/run_tests.sh --filter=<affected-test>` passes.
2. Full `./e2e/run_tests.sh` passes (no regressions).
3. `cargo test` passes (sanity check for any unit-level coupling).

## Out of Scope

- Adding new tests (Category B in brainstorming).
- Documentation/comment additions (Category C).
- Chapter-4/8 expansion or normative-granularity tests (Category D).
- TODO.md updates happen in the same commit-set but are not design decisions.
