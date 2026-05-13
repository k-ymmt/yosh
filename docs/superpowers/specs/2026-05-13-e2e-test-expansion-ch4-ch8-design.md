# E2E Test Expansion: Chapter 4 Utilities + Chapter 8 Environment Variables

**Date:** 2026-05-13
**Scope:** TODO.md `Future: E2E Test Expansion` L112 (横展開: Ch4 + Ch8)
**Status:** Design

This spec covers the **breadth-first** E2E expansion: systematic
coverage of POSIX **XCU §2.14 Special Built-In Utilities**, **XCU
§1.4 Required Built-Ins**, and **XBD §8 Environment Variables**.

The companion item (TODO.md L113 — Chapter 2 normative-requirement
深堀り) is **out of scope** and stays as a separate TODO.

## 1. Goals / Non-Goals

### Goals

- Add option-matrix-exhaustive E2E coverage for **XCU §2.14 Special
  Built-Ins** (15 commands).
- Add option-matrix-exhaustive E2E coverage for **XCU §1.4 Required
  Built-Ins** marked `Execution: built-in` (17 commands).
- Add coverage for **XBD §8 Environment Variables** standard
  variables that shells read/write/interpret (~25 variables).
- Register all known gaps (unimplemented builtins, deviations,
  harness-limited cases) with `XFAIL` so compliance gaps remain
  visible and `XPASS` becomes the natural completion signal.
- Reuse the existing `POSIX_REF` / `XFAIL` harness in
  `e2e/run_tests.sh` with **no harness feature changes** required.

### Non-Goals

- Chapter 2 normative-requirement granularity expansion (separate
  TODO L113).
- Implementing missing builtins (`read`, `getopts`, `pwd`, `type`,
  `hash`, `ulimit`). Tests are written against the POSIX-expected
  behavior and marked `XFAIL` until implementation lands.
- Coverage for external utilities outside XCU §1.4 built-in list
  (e.g., `grep`, `awk`, `sed`, `sort`).
- Performance or benchmark tests.
- Adding new harness metadata fields. New `POSIX_REF` shapes do not
  count as harness changes — they are accepted by the existing
  regex.

## 2. Coverage Matrix

Total estimate: **~264–360 new tests** across 3 phases. Of these,
~50–80 are expected to be `XFAIL` (mostly Phase 2 unimplemented
builtins and Phase 3 locale/mail variables).

### Phase 1: Special Built-In Utilities (XCU §2.14)

15 commands, all yosh-implemented today. Target ~119–150 tests in
`e2e/posix_spec/4_special_builtin/`.

| Builtin | Tests | Key verifications |
|---|---|---|
| `break [n]` | 6–8 | no-arg, depth-n, outside-loop error, invalid n |
| `: (colon)` | 3–4 | ignores args, `$? = 0`, complex args |
| `continue [n]` | 6–8 | no-arg, depth-n, outside-loop error |
| `. (dot) file` | 8–10 | PATH search, no-arg error, missing file, var-set continuation, `$?` propagation |
| `eval [args...]` | 8–10 | arg concat, recursion, empty args, syntax-error propagation |
| `exec [cmd]` | 8–10 | no-cmd (redirections persist), exec replaces shell, `$? = 127` not-found |
| `exit [n]` | 5–7 | no-arg (`$?` inherited), explicit n, out-of-range, invalid value |
| `export [-p] name[=val]...` | 10–12 | no-arg listing, `-p` form, single, multiple, empty value, readonly conflict, invalid identifier |
| `readonly [-p] name[=val]...` | 10–12 | no-arg listing, `-p` form, single, multiple, post-readonly assignment error |
| `return [n]` | 6–8 | outside function/dot, no-arg, explicit n |
| `set [-opts] [args]` | 18–22 | `-e`/`-u`/`-x`/`-n`/`-f`/`-C`/`-h`/`-m`/`-o name` each, `--`, positional params |
| `shift [n]` | 6–8 | one-by-one, explicit n, n-too-large error |
| `times` | 3–4 | output shape (4 values, `mm:ss.ff`) |
| `trap [action] [signals]` | 12–15 | set, reset, `-` form, multiple sigs, EXIT, ERR, subshell inheritance |
| `unset [-fv] names` | 10–12 | `-v`, `-f`, both, readonly conflict, undefined name |

**Sub-phase grouping (for the implementation plan):**

1. Control flow: `break` / `continue` / `return` / `exit` / `:` — ~26–35 tests
2. Scope & assignment: `export` / `readonly` / `unset` / `shift` / `set` — ~50–66 tests
3. Execution & substitution: `eval` / `exec` / `.` / `times` / `trap` — ~37–49 tests

### Phase 2: Required Built-Ins (XCU §1.4 "Execution: built-in")

17 commands. Target ~98–132 tests in
`e2e/posix_spec/4_required_builtin/`.

| Builtin | yosh status | Tests | XFAIL plan |
|---|---|---|---|
| `alias [name[=value]]...` | implemented | 8–10 | — |
| `bg [job_id...]` | implemented | 5–7 | non-monitor cases per existing pattern |
| `cd [-LP] [dir]` | implemented (13 existing tests in `e2e/builtin/`) | +8–10 supplement | — |
| `command [-pvV] cmd` | implemented (9 existing tests in `e2e/builtin_command/`) | +5–7 supplement | — |
| `fc [-rs] [-e ed] [first[last]]` | implemented | 8–10 | possibly partial XFAIL |
| `fg [job_id]` | implemented | 5–7 | same pattern as `bg` |
| `getopts optstring var [args]` | **not implemented** | 8–10 | full XFAIL |
| `hash [-r] [cmd]` | **not implemented** | 4–6 | full XFAIL |
| `jobs [-lp] [job_id]` | implemented | 6–8 | — |
| `kill [-s sig\|-N] [-l]` | implemented | +6–8 supplement | — |
| `pwd [-LP]` | **not implemented as builtin** | 4–6 | full XFAIL |
| `read [-r] var...` | **not implemented** | 8–10 | full XFAIL |
| `type name...` | **not implemented** | 5–7 | full XFAIL |
| `ulimit [-f] [num]` | **not implemented** | 3–5 | full XFAIL |
| `umask [-S] [mode]` | implemented | 6–8 | `-S` verification needed |
| `unalias [-a] name...` | implemented | 4–6 | — |
| `wait [pid...]` | implemented | 5–7 | — |

**Sub-phase grouping:**

1. Job control: `alias` / `unalias` / `bg` / `fg` / `jobs` / `wait` / `kill` — ~38–52 tests
2. Navigation & history: `cd` supplement / `pwd` (XFAIL) / `fc` — ~20–26 tests
3. Command lookup & type: `command` supplement / `type` (XFAIL) / `hash` (XFAIL) — ~14–20 tests
4. Unimplemented: `getopts` / `read` / `ulimit` — ~19–25 tests, all XFAIL
5. File: `umask` — ~6–8 tests

### Phase 3: Environment Variables (XBD §8)

~25 variables. Target ~47–78 tests in `e2e/posix_spec/8_env_vars/`.

| Group | Variables | Tests |
|---|---|---|
| Shell behavior | `HOME` / `IFS` / `PATH` / `PWD` / `OLDPWD` / `CDPATH` / `ENV` / `SHELL` | 16–24 |
| Prompt | `PS1` / `PS2` / `PS4` | 6–9 (non-interactive aspects only) |
| Special params | `LINENO` / `PPID` / `OPTARG` (XFAIL) / `OPTIND` (XFAIL) | 8–12 |
| Locale | `LANG` / `LC_ALL` / `LC_CTYPE` / `LC_COLLATE` / `LC_MESSAGES` / `NLSPATH` | 6–12 (mostly XFAIL: locale not implemented) |
| Mail | `MAIL` / `MAILCHECK` / `MAILPATH` | 3–9 (all XFAIL) |
| History & fc | `HISTFILE` / `HISTSIZE` / `FCEDIT` | 6–9 |
| Temp | `TMPDIR` | 2–3 |

**Sub-phase grouping** mirrors the row groups above.

## 3. Directory Structure and File Naming

### Layout

```
e2e/posix_spec/
├── 4_special_builtin/          ← Phase 1
│   ├── break_no_arg.sh
│   ├── break_with_n.sh
│   ├── break_outside_loop.sh
│   ├── colon_returns_zero.sh
│   ├── dot_path_search.sh
│   ├── eval_concat_args.sh
│   ├── ...
│   ├── set_opt_e.sh
│   ├── set_opt_u.sh
│   ├── trap_exit.sh
│   ├── trap_subshell_inheritance.sh
│   └── unset_fv.sh
├── 4_required_builtin/         ← Phase 2
│   ├── alias_define.sh
│   ├── bg_resumes_stopped.sh
│   ├── cd_phys_default.sh
│   ├── command_dash_p.sh
│   ├── ...
│   ├── getopts_basic.sh        (XFAIL)
│   ├── read_basic.sh           (XFAIL)
│   └── wait_pid.sh
└── 8_env_vars/                 ← Phase 3
    ├── HOME_default.sh
    ├── HOME_tilde_expansion.sh
    ├── IFS_default_unset.sh
    ├── IFS_field_split.sh
    ├── PATH_search.sh
    ├── PS1_default.sh
    ├── LINENO_in_function.sh
    └── ...
```

### Naming Convention

- `<command_or_var>_<aspect>.sh` (snake_case).
- File name should make intent clear without opening (e.g.,
  `set_opt_e.sh`, `trap_subshell_inheritance.sh`).
- Special-character names: `command -V` → `command_V_*.sh`, `[` →
  `bracket_*.sh`, `:` → `colon_*.sh`, `.` → `dot_*.sh`.

### `POSIX_REF` Format

- Phase 1: `POSIX_REF: 2.14.<NN> <command>` — matches existing
  Chapter 2.14 tests (`2.14.13 times`, `2.14.14 trap` already in
  use). The XCU §2.14 section number determines `NN`.
- Phase 2: `POSIX_REF: 4 Utilities - <name>` — matches existing
  `e2e/builtin/test_*.sh` and `e2e/builtin_command/` files.
- Phase 3: `POSIX_REF: 8 Environment Variables - <var>` — **new
  shape**. Must be added to `e2e/README.md` "POSIX_REF Format
  Contract" section.

The harness `run_tests.sh` parses `POSIX_REF` as a free-text label
and does not enforce its grammar, so adding a new shape requires
only documentation, no code change.

### Existing Test Handling

- `e2e/builtin/cd_*.sh` (13 files) and `e2e/builtin_command/` (9
  files) are **not moved**. Rationale: preserving `git blame` /
  `git log` continuity for those tests outweighs any consistency
  benefit from relocation.
- New supplementary tests go into the new `4_required_builtin/`
  directory. Duplication is essentially zero — existing tests are
  base cases, new tests add option-matrix breadth.
- Cleanup / consolidation of the parallel directories is **a
  separate TODO**, not part of this expansion.

## 4. Test Conventions

### Template

```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -f removes function, leaves variable of same name intact
# EXPECT_OUTPUT: var-value
# EXPECT_EXIT: 0
foo() { echo function; }
foo=var-value
unset -f foo
echo "$foo"
```

### XFAIL Convention

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read -r preserves backslashes in input
# EXPECT_OUTPUT: a\nb
# EXPECT_EXIT: 0
# XFAIL: read builtin not yet implemented (TODO: implement read)
printf 'a\\nb\n' | read -r line && echo "$line"
```

XFAIL reasons follow one of these prefixes for grep-ability when
implementation eventually catches up:

- `not yet implemented (TODO: ...)` — for missing builtins / options.
- `non-POSIX deviation (...)` — for known behavior that diverges
  intentionally.
- `harness limitation (...)` — when the test setup itself blocks
  verification (PTY-only, locale, etc.).

### Option Matrix Design

Each builtin's test set is organized in **3 layers**:

1. **Base layer (1–2 tests)** — default behavior with no/minimal args.
2. **Option layer (1–2 tests per option)** — every POSIX-documented
   option.
3. **Edge-case layer (2–5 tests)** — invalid args, interactions,
   `$?` after error, subshell / function-scope behavior.

Example for `export` (10–12 tests):

- Base: no-arg listing, single `export NAME`.
- Option: `-p` form, `--`.
- Assignment: `export A=1`, multiple `export A=1 B=2`, empty `export A=`.
- Interaction: child-process inheritance, `readonly` conflict,
  undefined variable.
- Error: invalid identifier `export 1FOO=v`.

### Verification Strategy

- **stdout** — `EXPECT_OUTPUT` exact match (existing harness
  contract).
- **exit code** — `EXPECT_EXIT` always explicit (do not omit even
  for `0`; the POSIX-expected exit must be visible in the test
  metadata).
- **stderr** — `EXPECT_STDERR` substring match for expected error
  messages.
- **side effects** — echo the affected variable / `$?` to stdout and
  match via `EXPECT_OUTPUT`.
- **environment isolation** — file-touching tests use
  `$TEST_TMPDIR` (auto-cleaned by the harness).

### Conventions Already in Place

- `EXPECT_OUTPUT:` empty-form silent-skip was fixed on 2026-05-13
  (commit `51d147e`), so tests expecting empty stdout can use
  `# EXPECT_OUTPUT:` without surprise behavior.
- The Chapter 4 `POSIX_REF` shape is established and documented in
  `e2e/README.md`.

## 5. Phasing and Acceptance Criteria

### Phase 1: Special Built-In Utilities

- New dir: `e2e/posix_spec/4_special_builtin/`
- ~119–150 tests added across the 3 sub-phases above.
- Acceptance:
  - `./e2e/run_tests.sh --filter=4_special_builtin` is all PASS or
    XFAIL (no FAIL / XPASS).
  - `e2e/README.md` already documents the `2.X.Y` shape; no doc
    change needed unless a sub-shape (e.g., option-specific) emerges.
  - TODO.md L112 is **not yet removed** — it remains until all 3
    phases land.

### Phase 2: Required Built-Ins

- New dir: `e2e/posix_spec/4_required_builtin/`
- ~98–132 tests, with ~30–40 expected `XFAIL` from the 6
  unimplemented commands (`getopts` / `hash` / `pwd` / `read` /
  `type` / `ulimit`).
- Acceptance:
  - All-PASS or all-XFAIL on the new directory.
  - TODO.md gains a new section listing the unimplemented builtin
    surface area as concrete future work, pointing back to the
    XFAIL tests as the spec for what each implementation must
    pass. Format: `XCU §1.4 required builtin '<name>' not yet
    implemented — XFAIL tests in e2e/posix_spec/4_required_builtin/<name>_*.sh
    document the expected behavior`.

### Phase 3: Environment Variables

- New dir: `e2e/posix_spec/8_env_vars/`
- ~47–78 tests, with PASS / XFAIL roughly split (locale + mail vars
  all XFAIL).
- Acceptance:
  - All-PASS or all-XFAIL on the new directory.
  - `e2e/README.md` "POSIX_REF Format Contract" section gains a
    bullet for `8 Environment Variables - <var>`.
  - **TODO.md L112 is deleted** (per the project's "delete
    completed items, do not use `[x]`" convention in
    `CLAUDE.md`). L113 (Chapter 2 深堀り) remains.

### Cross-Phase Acceptance

- `cargo test` (unit + integration): all PASS — no regression.
- `./e2e/run_tests.sh`: existing 398 tests retain their previous
  PASS/XFAIL status. Total test count climbs to ~660–760.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --
  -D warnings` clean (test file additions only, no library code
  changes expected).

## 6. Risks and Mitigations

| Risk | Mitigation |
|---|---|
| `2.14.NN` `POSIX_REF` duplication with existing tests | Test identity is `POSIX_REF` + file path. Coexistence is fine; the harness does not enforce `POSIX_REF` uniqueness. |
| `set -o vi` / `set -o emacs` need an interactive shell | Limit `set` option matrix to non-interactive options (`-e` / `-u` / `-x` / `-n` / `-f` / `-C` / `-h` / `-m` / `-o noclobber|errexit|nounset|...`). Interactive-only options become XFAIL with `harness limitation` reason. |
| `trap` signal-delivery tests can flake under load | Use `kill -s <sig> $$` inside a subshell + immediate wait pattern, with `EXIT` traps preferred where the behavior is equivalent. Avoid timer-based signal arrival. |
| XFAIL noise grows unbounded | Standardize the XFAIL reason prefix (§4) so future grep can produce a clean implementation-priority list. |
| Phase 1 is the largest sub-batch and may feel slow | Sub-phases 1.1 / 1.2 / 1.3 are independently commit-shippable, giving 3 commit milestones inside Phase 1. |
| `XPASS` (XFAIL test unexpectedly passing) breaks CI | The harness already classifies XPASS as a separate result; treat XPASS as a signal to remove the `XFAIL:` line, not as a build failure. Confirm this matches `run_tests.sh` current exit policy in the implementation plan's first verification step. |
| `pwd` is not a yosh builtin but `cd` updates `$PWD` — semantic overlap | Phase 2 tests target the `pwd` *builtin*, not the variable. The variable is covered in Phase 3 (`PWD` env var). The tests will not conflict. |
| `EXPECT_OUTPUT` empty-form behavior assumption | Already fixed in `51d147e` (2026-05-13). No further action required. |

## 7. Open Questions

None at design time. The matrix size (~264–360 tests) is large but
each individual test is small (~5–15 LOC). Execution risk is low
because no production code changes.

## 8. Out of Scope (Explicit)

- Implementing missing builtins (`read`, `getopts`, `pwd`, `type`,
  `hash`, `ulimit`). The XFAIL tests added here serve as the
  acceptance spec for each future implementation.
- Refactoring or relocating existing `e2e/builtin/` and
  `e2e/builtin_command/` tests.
- Adding `EXPECT_STDOUT_REGEX` or other new harness metadata.
- Coverage for XBD §8 variables that are read by external utilities
  but not interpreted by the shell (e.g., `EDITOR`, `VISUAL`
  outside the `fc` interaction, `PAGER`).
- Chapter 2 normative-requirement granularity expansion — that work
  belongs to TODO.md L113.

## 9. Next Steps

Upon spec approval:

1. Invoke `writing-plans` skill to produce the implementation plan
   covering all 3 phases as sub-phases within a single rolling
   plan.
2. Implementation begins at Phase 1 sub-phase 1 (control flow:
   `break` / `continue` / `return` / `exit` / `:`).
