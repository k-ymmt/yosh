# E2E Quick Wins — TODO.md L112 / L116 / L124 / L125

**Date:** 2026-05-12
**Status:** Approved (design phase)
**Source TODOs:** `TODO.md` lines 112, 116, 124, 125

## 1. Purpose

Address four small, independent cleanups under TODO.md's `Future: E2E Test
Expansion` and `Future: Release Skill Enhancements` sections in a single
batch. None of these change shell behavior; they tighten metadata accuracy,
relocate misfiled tests, and document a latent runner race.

Out of scope: L113 (`fd_close.sh` improvement), L114 (Chapter 4/8 expansion),
L115 (normative-requirement granularity), L117 (`$0` divergence).

## 2. Background

### L112 — `POSIX_REF` granularity

Fifteen files in `e2e/builtin/` cite the generic
`POSIX_REF: 2.14 Special Built-In Utilities`. POSIX §2.14 has 15 numbered
subsections (2.14.1–2.14.15), one per special builtin. The existing
`e2e/posix_spec/2_14_13_times/` directory already uses the precise
`2.14.13 times` form. Aligning the rest makes `POSIX_REF` greppable per
subsection.

### L116 — `2_14_test/` mis-citation and relocation

The 15 files under `e2e/posix_spec/2_14_test/` cite
`POSIX_REF: 2.14 test`, but POSIX §2.14 is "Special Built-In Utilities" and
does **not** include `test`. The `test` utility is in POSIX XCU Chapter 4
(Utilities). The directory is therefore misfiled twice: wrong section
number and wrong parent directory (`posix_spec/` is currently Chapter 2
only).

The chosen target is `e2e/builtin/` (flat move). `e2e/builtin/` already
hosts Chapter 4 utilities that are also shell builtins (e.g.,
`cd_basic.sh` with `POSIX_REF: 4 Utilities - cd`), so the move follows
existing precedent. Filenames (`test_*.sh`) already match the
`<builtin>_<aspect>.sh` convention.

### L124 — Watchdog race-condition documentation

`e2e/run_tests.sh:290-292` spawns a single-shot `sleep $TIMEOUT &&
kill -9` watchdog. If the test exits at roughly the same instant the
timer fires, `kill -9` returns `ESRCH` and the `"timeout"` marker file is
not written. Behavior remains correct because `wait $_pid` (line 297)
already captured the real exit code; the `_exit_file` branch is purely
diagnostic. The race is benign but undocumented today.

### L125 — `YOSH_E2E_NO_TIMEOUT` help text

Current `--help` text says "local use only" — too soft. The flag, if set
in CI or in `release.sh`, lets a single runaway test hang the entire
suite indefinitely. The wording should make that consequence explicit.

## 3. Changes

### 3.1 L112 — Rewrite generic §2.14 `POSIX_REF` lines

In each file below, replace the second line:

`# POSIX_REF: 2.14 Special Built-In Utilities` → new value.

| File | New `POSIX_REF` |
|---|---|
| `e2e/builtin/colon_noop.sh` | `2.14.2 colon` |
| `e2e/builtin/source_file.sh` | `2.14.4 dot` |
| `e2e/builtin/eval_basic.sh` | `2.14.5 eval` |
| `e2e/builtin/eval_variable.sh` | `2.14.5 eval` |
| `e2e/builtin/exec_no_args.sh` | `2.14.6 exec` |
| `e2e/builtin/exec_replace.sh` | `2.14.6 exec` |
| `e2e/builtin/export_basic.sh` | `2.14.8 export` |
| `e2e/builtin/export_format.sh` | `2.14.8 export` |
| `e2e/builtin/readonly_basic.sh` | `2.14.9 readonly` |
| `e2e/builtin/set_dash_dash.sh` | `2.14.11 set` |
| `e2e/builtin/set_monitor_flag.sh` | `2.14.11 set` |
| `e2e/builtin/set_positional.sh` | `2.14.11 set` |
| `e2e/builtin/shift_basic.sh` | `2.14.12 shift` |
| `e2e/builtin/unset_readonly_error.sh` | `2.14.15 unset` |
| `e2e/builtin/unset_variable.sh` | `2.14.15 unset` |

Notes:
- `source_file.sh` tests `. file` (the POSIX dot builtin). `source` is a
  bash/ksh extension; the POSIX citation must be `2.14.4 dot`.
- `set_monitor_flag.sh` exercises `set -m` / `set +m`. The option's
  *semantics* belong to §2.11 Job Control, but the *builtin* is §2.14.11.
  We keep the citation on the builtin (matches the sibling
  `set_monitor_off.sh` decision to cite `2.11 Job Control`, distinguishing
  builtin-level vs option-semantics tests).

### 3.2 L116 — Relocate `2_14_test/` into `e2e/builtin/`

1. `git mv e2e/posix_spec/2_14_test/test_*.sh e2e/builtin/` (15 files).
2. In each moved file, replace `# POSIX_REF: 2.14 test` with
   `# POSIX_REF: 4 Utilities - test`. (Mirrors the
   `4 Utilities - cd` style already used by `cd_*.sh`.)
3. Remove the now-empty directory: `rmdir e2e/posix_spec/2_14_test`.
4. Verify no source/docs reference the old path:
   `grep -r 'posix_spec/2_14_test\|2_14_test/' e2e/ src/ tests/ docs/superpowers/specs/`
   (the `target/package/` snapshots are publish-time artifacts and are
   ignored).

Files moved (15):
`test_bracket_requires_closing.sh`, `test_file_exists.sh`,
`test_file_readable.sh`, `test_file_regular.sh`, `test_file_symlink.sh`,
`test_integer_compare.sh`, `test_integer_parse_error.sh`,
`test_isatty_fd.sh`, `test_negation.sh`, `test_no_args.sh`,
`test_paren_grouping.sh`, `test_string_eq_neq.sh`,
`test_string_nonempty.sh`, `test_too_many_args.sh`,
`test_unknown_operator.sh`.

### 3.3 L124 — Document the watchdog race

In `e2e/run_tests.sh`, immediately above the existing `(` on line 290
(the timer subshell), insert:

```sh
        # Single-shot watchdog: SIGKILL the test if it outlives $TIMEOUT.
        # Benign race — if the test exits just as the timer fires, kill -9
        # returns ESRCH and we skip writing the "timeout" marker. The exit
        # code from `wait $_pid` below is the authoritative result; the
        # marker branch is diagnostic only, so the race cannot corrupt
        # pass/fail accounting.
```

Indentation matches the surrounding `else` arm (8 spaces). No executable
code changes.

### 3.4 L125 — Sharpen `YOSH_E2E_NO_TIMEOUT` help text

Replace `e2e/run_tests.sh:116`:

```sh
    printf "  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout (local use only)\n"
```

with:

```sh
    printf "  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout; never set in CI or\n"
    printf "                         release.sh (individual runaway tests will hang forever)\n"
```

The second line aligns continuation text under the description column to
match the visual flow of the other entries.

### 3.5 TODO.md cleanup

After the changes above land, delete (do not mark `[x]`) the four bullets
from TODO.md:

- L112: "Builtin test POSIX_REF values could use more specific section numbers ..."
- L116: "`e2e/posix_spec/2_14_test/` POSIX_REF mis-citation ..."
- L124: "e2e runner timer race-condition comment ..."
- L125: "`YOSH_E2E_NO_TIMEOUT` help wording ..."

Per project convention (`CLAUDE.md`), completed items are removed rather
than checked off.

## 4. Verification

Done in order:

1. **`cargo build`** — debug build for the runner to invoke.
2. **`./e2e/run_tests.sh --filter=builtin`** — exercises the 15 moved
   `test_*.sh` files (now in `e2e/builtin/`) plus the 15 files whose
   `POSIX_REF` changed. All 30 must remain `[PASS]` with no new
   `[FAIL]` / `[TIME]`.
3. **`./e2e/run_tests.sh --help`** — visually confirm the
   `YOSH_E2E_NO_TIMEOUT` help text reads correctly and wraps under the
   description column.
4. **`grep -RE 'POSIX_REF: 2\.14 Special Built-In Utilities' e2e/`** —
   expected: no matches.
5. **`grep -RE 'POSIX_REF: 2\.14 test' e2e/`** — expected: no matches.
6. **`grep -RE 'posix_spec/2_14_test' e2e/ src/ tests/ docs/superpowers/specs/`** —
   expected: no matches.
7. Confirm `e2e/posix_spec/2_14_test/` no longer exists.

Stretch (optional, for confidence): run the full
`./e2e/run_tests.sh` and verify totals match the pre-change run modulo
the moved tests (15 fewer under `posix_spec/`, 15 more under `builtin/`).

## 5. Risk and Rollback

Risk is low — all changes are textual and confined to E2E metadata, one
shell-runner comment, and one help-text line. No `src/` or `tests/`
(Rust) changes. No CI plumbing changes.

Rollback is `git revert <commit>` if needed; nothing else depends on the
new POSIX_REF strings or the new file locations.

## 6. Non-goals

- No changes to test *content* (the body of any `.sh` file stays the
  same).
- No expansion of E2E coverage (L113/L114/L115 deferred).
- No introduction of a `posix_spec/4_*/` convention (rejected during
  brainstorming in favor of the existing `e2e/builtin/` flat layout).
- No CI workflow changes (separate TODO under "Code Format Drift").
