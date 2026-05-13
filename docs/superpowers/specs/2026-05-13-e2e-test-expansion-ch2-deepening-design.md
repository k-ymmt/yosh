# E2E Test Expansion: Chapter 2 Normative-Clause Deepening

**Date:** 2026-05-13
**Scope:** TODO.md `Future: E2E Test Expansion` (Chapter 2 normative-requirement granularity)
**Status:** Design

This spec covers the **depth-first** E2E expansion of POSIX XCU
Chapter 2 (Shell Command Language). Each `shall` / `must` / `should`
clause receives at least one dedicated E2E test, with `XFAIL`
registering unimplemented or deviating cases.

The companion item (Chapter 4 + Chapter 8 breadth expansion) is
already complete (see
`docs/superpowers/specs/2026-05-13-e2e-test-expansion-ch4-ch8-design.md`)
and is out of scope here.

## 1. Goals / Non-Goals

### Goals

- Cover **POSIX XCU Chapter 2 §2.1–§2.13** at normative-clause
  granularity — each `shall` / `must` / `should` clause maps to at
  least one E2E test file.
- Add ~140–210 new tests under `e2e/posix_spec/2_*/`.
- Register all yosh deviations / unimplemented cases as `XFAIL` so
  compliance gaps remain visible and `XPASS` becomes the natural
  completion signal.
- Reuse existing `POSIX_REF` shapes — no new harness formats added.
- Reuse `e2e/run_tests.sh` with **no harness feature changes**.

### Non-Goals

- §2.14 Special Built-In Utilities — completed in the Ch4+Ch8 spec
  (`4_special_builtin/`).
- Refactoring / relocating existing `e2e/{command_execution,
  variable_and_expansion, arithmetic, command_substitution,
  pipeline_and_list, redirection, control_flow, function, builtin,
  signal_and_trap, subshell, quoting, field_splitting}/` tests.
  Duplication with new `posix_spec/2_*/` tests is acceptable.
- Refactoring / consolidating existing `e2e/posix_spec/2_*/` tests
  (parallel coexistence is fine).
- Implementing missing builtins / features (e.g. function-local
  variables, advanced parameter expansion edge cases). The XFAIL
  tests added here serve as the acceptance spec for each future
  implementation.
- Harness extensions (`EXPECT_STDOUT_REGEX`, parallel execution,
  locale setup, etc.).
- Performance or benchmark tests.
- Coverage for non-XCU chapters (XBD §8 is already covered by the
  Ch4+Ch8 spec).
- New CI workflows (macOS runner etc. tracked separately).

## 2. Coverage Matrix

Total estimate: **~142–209 new tests** across 4 phases. Of these,
~30–50 are expected to be `XFAIL` (advanced PE flags, here-doc
deviations, function-local scope absence, etc.). Existing test
counts in parentheses are not changed by this work.

### Phase 1 — Shell Introduction, Quoting, Parameters (~28–40 tests)

| § | Scope | Existing | New |
|---|---|---|---|
| 2.1 Shell Introduction | shell-startup minimal contract (`#!`, no-args invocation), testable normatives | 0 | 2–3 |
| 2.2 Quoting | 2.2.1 Escape, 2.2.2 Single-Quote, 2.2.3 Double-Quote — each subsection's `shall` clauses | 0 | 12–18 |
| 2.5.1 Positional Parameters | `$1`–`$9`, `${10}`, `set -- ...`, `$*` vs `$@` | 0 | 6–8 |
| 2.5.2 Special Parameters | `$#`, `$?`, `$-`, `$!`, `$0`, `$$` (special semantics, not collision with §8 env vars) | 0 | 6–8 |
| 2.5.3 Shell Variables (supplement) | minor gaps not in existing 13 tests | (13) | (+2–3) |

### Phase 2 — Word Expansion (~48–66 tests)

All seven §2.6 subsections. Commits split per subsection.

| § | Scope | Existing | New |
|---|---|---|---|
| 2.6.1 Tilde Expansion (supplement) | edge cases not in existing 23 tests | (23) | (+3–5) |
| 2.6.2 Parameter Expansion | `${var}`, `${var:-w}`, `${var:=w}`, `${var:?w}`, `${var:+w}`, `${var-w}`, `${var=w}`, `${var?w}`, `${var+w}`, `${var%pat}`, `${var%%pat}`, `${var#pat}`, `${var##pat}`, `${#var}` — each form's normative behavior | 0 | 16–22 |
| 2.6.3 Command Substitution | `$(...)` vs backquote, nesting, quoting context, exit status propagation | 0 | 6–8 |
| 2.6.4 Arithmetic Expansion | `$((...))`, expansion ordering, error behavior, signed/unsigned | 0 | 6–8 |
| 2.6.5 Field Splitting | IFS whitespace vs non-whitespace, `$@` / `$*` exceptions, unset IFS | 0 | 8–10 |
| 2.6.6 Pathname Expansion | `*` / `?` / `[]`, quoted patterns, no-match behavior, dot-file exclusion | 0 | 6–8 |
| 2.6.7 Quote Removal | quote-removal order after other expansions | 0 | 3–5 |

### Phase 3 — Redirection (rest), Errors, Commands, Signals, ExecEnv (~51–78 tests)

| § | Scope | Existing | New |
|---|---|---|---|
| 2.7.1–2.7.4 input/output redirection | `<` / `>` / `>>` / `<>` and combinations (existing dir covers dup forms only) | (15) | (+8–12) |
| 2.7.5 Appending | `>>` create-vs-append, `noclobber` interaction | 0 | 3–4 |
| 2.7.6 Here-Document | quoted/unquoted delimiter, `<<-` tab strip, variable expansion in body | 0 | 6–8 |
| 2.7.7 Duplicating | covered by existing dup_*.sh; supplement minimal | (10) | (+0–2) |
| 2.7.8 Open R/W | covered by existing readwrite_*.sh | (4) | (+0–1) |
| 2.8 Exit Status / Errors (supplement) | special-builtin error semantics, `$?` propagation across constructs | (3) | (+5–8) |
| 2.9.1 Simple Commands | 4-step execution order (redir → assign → expand → execute) | 0 | 4–6 |
| 2.9.2 Pipelines | `\|`, `!` negation, last-command status semantics | 0 | 4–5 |
| 2.9.3 Lists | `;`, `&`, `&&`, `\|\|`, precedence | 0 | 4–6 |
| 2.9.4 Compound Commands | `( ... )`, `{ ...; }`, `for`, `case` syntax (patterns covered in §2.13), `if`, `while` / `until` | 0 | 6–10 |
| 2.9.5 Function Definition | `name() { ... }`, scope rules, `return` interaction | 0 | 4–5 |
| 2.11 Signals (supplement) | trap inheritance, `kill -l`, `trap -p` | (3) | (+3–5) |
| 2.12 Shell Execution Environment | env inheritance, export propagation, subshell isolation | 0 | 4–6 |

### Phase 4 — Existing § Supplement (~15–25 tests)

| § | Scope | Existing | New |
|---|---|---|---|
| 2.3 Token Recognition (supplement) | normative clauses not in existing 3 tests (quote interior, escape detail) | (3) | (+4–6) |
| 2.4 Reserved Words (supplement) | reserved words in non-word position, remaining literal cases | (4) | (+3–5) |
| 2.10 Shell Grammar (supplement) | grammar `shall` clauses not in existing 39 tests | (39) | (+4–8) |
| 2.13 Pattern Matching (supplement) | `[!...]`, `[a-z]`, bracket-class edge cases | (5) | (+4–6) |

### Totals

| Phase | Existing | New |
|---|---|---|
| P1 | 13 | 28–40 |
| P2 | 23 | 48–66 |
| P3 | 35 | 51–78 |
| P4 | 51 | 15–25 |
| **Total** | **122** | **~142–209** |

## 3. Directory Structure and File Naming

### Layout

No relocation. New directories are added per subsection.

```
e2e/posix_spec/
├── 2_01_shell_introduction/        ← new (P1)
├── 2_02_quoting/                   ← new (P1)
├── 2_03_token_recognition/         ← existing, supplement only
├── 2_04_reserved_words/            ← existing, supplement only
├── 2_05_01_positional_params/      ← new (P1)
├── 2_05_02_special_params/         ← new (P1)
├── 2_05_03_shell_variables/        ← existing, supplement only
├── 2_06_01_tilde_expansion/        ← existing, supplement only
├── 2_06_02_parameter_expansion/    ← new (P2)
├── 2_06_03_command_substitution/   ← new (P2)
├── 2_06_04_arithmetic_expansion/   ← new (P2)
├── 2_06_05_field_splitting/        ← new (P2)
├── 2_06_06_pathname_expansion/     ← new (P2)
├── 2_06_07_quote_removal/          ← new (P2)
├── 2_07_redirection/               ← existing, supplement only
├── 2_07_05_appending/              ← new (P3, may merge into 2_07_redirection if minimal)
├── 2_07_06_heredoc/                ← new (P3)
├── 2_08_01_consequences_of_shell_errors/  ← existing, supplement only
├── 2_09_01_simple_commands/        ← new (P3)
├── 2_09_02_pipelines/              ← new (P3)
├── 2_09_03_lists/                  ← new (P3)
├── 2_09_04_compound_commands/      ← new (P3)
├── 2_09_05_function_definition/    ← new (P3)
├── 2_10_1_lexical/                 ← existing
├── 2_10_shell_grammar/             ← existing, supplement only
├── 2_11_signals_and_error_handling/← existing, supplement only
├── 2_12_shell_exec_env/            ← new (P3)
├── 2_13_pattern_matching/          ← existing, supplement only
├── 2_14_13_times/                  ← existing (Ch4+Ch8 artifact, leave as-is)
├── 4_required_builtin/             ← existing (Ch4+Ch8)
├── 4_special_builtin/              ← existing (Ch4+Ch8)
└── 8_env_vars/                     ← existing (Ch4+Ch8)
```

### Naming Convention

- **Directory**: `2_NN_subsection_name/` or
  `2_NN_MM_subsubsection_name/` (snake_case).
- **File**: `<topic>_<aspect>.sh` (snake_case). Example:
  `escape_newline_continuation.sh`, `param_default_unset.sh`,
  `pipeline_negation_status.sh`.

### `POSIX_REF` Format

Only existing shapes (per `e2e/README.md` "POSIX_REF Format
Contract") are used:

- `POSIX_REF: 2.X.Y <Subsection Name>` — primary shape for ordinary
  references. E.g., `2.6.2 Parameter Expansion`, `2.9.1 Simple
  Commands`.
- `POSIX_REF: 2.10.2 Rule N - <Name>` and
  `POSIX_REF: 2.10 Shell Grammar - <Topic>` — used only in P4
  supplement to §2.10 to match existing tests in
  `2_10_shell_grammar/`.

No new shapes. `e2e/README.md` requires no changes.

### Existing Test Handling

- `e2e/{command_execution, variable_and_expansion, arithmetic,
  command_substitution, pipeline_and_list, redirection,
  control_flow, function, builtin, signal_and_trap, subshell,
  quoting, field_splitting}/` are **not moved**. Rationale:
  preserving `git blame` / `git log` continuity.
- Existing `e2e/posix_spec/2_*/` directories are **not moved**.
- Duplication is accepted. Consolidation is a separate TODO.

## 4. Test Conventions

### Template (PASS)

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-word} substitutes word when var is unset
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset x
echo "${x:-hello}"
```

### Template (XFAIL)

```sh
#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Multi-byte IFS character splits fields in UTF-8 locale
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
# XFAIL: non-POSIX deviation (multi-byte IFS not supported; ASCII byte-set only)
IFS=日
set -- a日b
printf '%s\n' "$@"
```

### XFAIL Reason Prefixes

Inherited from Ch4+Ch8 spec (§4):

| Prefix | Use |
|---|---|
| `not yet implemented (TODO: ...)` | missing builtins / features |
| `non-POSIX deviation (...)` | known intentional / unintentional divergence |
| `harness limitation (...)` | PTY-only, locale, signal timing, etc. |

### Normative-Clause Mapping

Goal: **one normative clause = one test file**. Each `DESCRIPTION`
line includes the POSIX clause keyphrase for grep-based traceability.
Examples:

- `DESCRIPTION: ${var:-word} substitutes word when var is unset`
  (POSIX: `shall substitute`)
- `DESCRIPTION: pipeline status is the last command's status`
  (POSIX: `shall reflect`)
- `DESCRIPTION: special builtin error causes non-interactive shell to exit`
  (POSIX: `shall exit`)

### 3-Layer Coverage Per § / Subsection

1. **Base layer (1–2 tests)** — core normative clause, default
   behavior.
2. **Clause layer (1 test per normative clause)** — each
   `shall` / `must` / `should` clause.
3. **Edge-case layer (2–5 tests)** — interaction with adjacent §,
   error semantics, `$?` propagation.

### Verification Strategy

- **stdout** — `EXPECT_OUTPUT` exact match.
- **exit code** — `EXPECT_EXIT` always explicit (even for `0`).
- **stderr** — `EXPECT_STDERR` substring match.
- **side effects** — echo the affected variable / `$?` to stdout
  and match via `EXPECT_OUTPUT`.
- **environment isolation** — file-touching tests use
  `$TEST_TMPDIR` (auto-cleaned by the harness).

### Conventions Already in Place

- `EXPECT_OUTPUT:` empty form does not silent-skip (2026-05-13
  commit `51d147e`).
- Test file permissions: `644` (CLAUDE.md).

### Expected XFAIL Distribution

| Reason | Likely § | Estimate |
|---|---|---|
| `not yet implemented` | 2.6.2 advanced PE forms, 2.7.6 here-doc detail, 2.9.5 function local scope, ... | ~15–25 |
| `non-POSIX deviation` | 2.8 break/continue/exec edge cases (related to existing 9 deviations), 2.11 async trap INT, 2.5.2 `$PPID` | ~10–15 |
| `harness limitation` | 2.11 SIGSTOP/CONT, 2.12 shell-exit trap, 2.1 startup contract | ~5–10 |
| **Total** | | **~30–50** |

Expected PASS:XFAIL ratio: **3:1 to 4:1**.

## 5. Phasing and Acceptance Criteria

### Phase / Sub-Phase Breakdown (commit boundaries)

```
P1 — Shell Introduction, Quoting, Parameters (~28–40 tests, 3 commits)
  P1.1: 2.1 + 2.2 Quoting                (~14–21 tests)
  P1.2: 2.5.1 Positional                 (~6–8 tests)
  P1.3: 2.5.2 Special + 2.5.3 supplement (~8–11 tests)

P2 — Word Expansion (~48–66 tests, 7 commits — one per subsection)
  P2.1: 2.6.1 supplement                 (~3–5 tests)
  P2.2: 2.6.2 Parameter Expansion        (~16–22 tests)
  P2.3: 2.6.3 Command Substitution       (~6–8 tests)
  P2.4: 2.6.4 Arithmetic Expansion       (~6–8 tests)
  P2.5: 2.6.5 Field Splitting            (~8–10 tests)
  P2.6: 2.6.6 Pathname Expansion         (~6–8 tests)
  P2.7: 2.6.7 Quote Removal              (~3–5 tests)

P3 — Redirection rest, Errors, Commands, Signals, ExecEnv (~51–78 tests, 5 commits)
  P3.1: 2.7 redirection rest + here-doc  (~17–27 tests)
  P3.2: 2.8 Errors supplement            (~5–8 tests)
  P3.3: 2.9 Shell Commands (all five)    (~22–32 tests)
  P3.4: 2.11 Signals supplement          (~3–5 tests)
  P3.5: 2.12 Shell Exec Env              (~4–6 tests)

P4 — Existing § supplement (~15–25 tests, 1–2 commits)
  P4.1: 2.3 / 2.4 / 2.10 / 2.13 supplement (~15–25 tests)
```

### Sub-Phase Acceptance (per commit)

- `./e2e/run_tests.sh --filter=<dir>`: all PASS or XFAIL (no FAIL,
  no XPASS).
- `cargo test`: no regression.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --
  -D warnings`: clean. Test-file additions only — no library
  changes expected.
- New XFAIL entries use one of the §4 reason prefixes.
- XPASS observation triggers `XFAIL:` line removal in the same
  commit (treat XPASS as an implicit-fix signal).

### Phase Acceptance

| Phase | Completion criterion |
|---|---|
| P1 done | sub-phase criteria × 3 + 4 new directories exist (`2_01_*`, `2_02_*`, `2_05_01_*`, `2_05_02_*`) |
| P2 done | sub-phase criteria × 7 + all seven 2.6.X subsection directories exist |
| P3 done | sub-phase criteria × 5 + `2_07_06_heredoc`, `2_09_0?_*`, `2_12_*` directories exist |
| P4 done | existing 4 directories supplemented + **TODO.md "Future: E2E Test Expansion" entry deleted** |

### Cross-Phase Acceptance (all phases complete)

- `./e2e/run_tests.sh`: existing tests (~660 currently) retain their
  previous PASS/XFAIL status.
- Total test count climbs to ~802–869.
- `cargo test`: all PASS.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets --
  -D warnings`: clean.
- TODO.md `Future: E2E Test Expansion` section is deleted.
- TODO.md `Future: POSIX Conformance Bugs` gains entries for new
  XFAIL items uncovered during the expansion (one bullet per
  unimplemented feature / deviation discovered).

### XPASS Handling

XPASS is **not** a CI failure — it's a signal:

- The harness classifies XPASS as a distinct result (existing).
- On XPASS, remove the `XFAIL:` line in the same commit so the test
  becomes a PASS.
- Remove the corresponding TODO.md entry (if any).

## 6. Risks and Mitigations

| Risk | Mitigation |
|---|---|
| 2.6.2 Parameter Expansion edge cases (e.g., `${var%pat}` patterns) yosh-unimplemented → bulk XFAIL | Accept XFAIL. Each XFAIL escalates as a `Future: POSIX Conformance Bugs` TODO entry, building an actionable backlog. |
| 2.6.6 Pathname Expansion / 2.13 Pattern Matching tests CWD-fragile | Run each in `$TEST_TMPDIR` via `cd "$TEST_TMPDIR"` prologue. |
| 2.7.6 here-document delimiter rules complex (quoted, unquoted, `<<-`, escaped) | Split per subcategory: `heredoc_quoted_delim.sh`, `heredoc_dash_strips_tab.sh`, `heredoc_unquoted_expands.sh`, etc. |
| 2.11 signal tests flake under load | Use `kill -s <sig> $$` + immediate-wait pattern. Avoid timer-based signal arrival. (Same approach as Ch4+Ch8 spec §6.) |
| 2.12 Shell Exec Env behavior differs macOS / Linux | Record platform-specific divergence as `non-POSIX deviation (platform-specific)` XFAIL; revisit when macOS CI runner lands. |
| Large sub-phase commits hard to review | P2.2 and P3.3 are largest. P2.2 splits naturally by PE family (`param_default_*.sh`, `param_assign_*.sh`, `param_error_*.sh`, `param_alt_*.sh`, `param_remove_suffix_*.sh`, `param_remove_prefix_*.sh`, `param_length_*.sh`). P3.3 splits by §2.9 subsection. |
| New nested directories under `posix_spec/` may surprise harness | `run_tests.sh` uses recursive glob and depth-independent traversal (verified via existing `2_10_1_lexical/`). |
| `2.5.2` `$-` option-letter order is undefined | Use `grep`-based verification inside the test body, not strict `EXPECT_OUTPUT`. |
| Duplication with existing `e2e/{quoting, redirection, ...}/` directories | Accepted by design. Test identity is `POSIX_REF` + file path; harness does not enforce uniqueness. |

## 7. Open Questions

### To resolve at sub-phase start (not blocking spec approval)

1. **`2.6.2` Parameter Expansion coverage depth.** Base set is the
   10 POSIX forms (`:-`, `:=`, `:?`, `:+`, `-`, `=`, `?`, `+`,
   `%`/`%%`, `#`/`##`, `${#var}`). Nested expansion (`${var:-${other:-x}}`),
   quoted-context behavior, and IFS interaction will be enumerated
   at P2.2 start. Tentative plan: include 5–8 such cases in the
   edge-case layer.

2. **`2.7.6` `<<-` tab-strip rule details.** POSIX specifies "tabs
   only". Mixed-space behavior to be confirmed against the POSIX
   text at P3.1 start. Tentative tests: tab-only strip, space-only
   no-strip, mixed boundary, escape interaction.

3. **`2.9.4` Compound Commands and §2.13 overlap.** `case` syntax
   (`)`, `;;`, no-fallthrough) lives in 2.9.4; pattern matching
   (`*`, `?`, `[]`) lives in 2.13. Boundary enforced in test
   `DESCRIPTION` lines.

4. **`2.5.2` `$-` option-letter ordering.** POSIX leaves the order
   unspecified. Tests use `grep -q` against `"$-"` rather than
   strict `EXPECT_OUTPUT` comparison.

5. **TODO.md follow-up entry placement.** New XFAIL-discovered
   issues are appended to the existing `## Future: POSIX
   Conformance Bugs` section in TODO.md (consistent with the
   2026-05-13 Ch4+Ch8 work).

## 8. Out of Scope (Explicit)

- §2.14 Special Built-In Utilities — completed in
  `4_special_builtin/` (Ch4+Ch8 spec).
- Refactoring / moving existing
  `e2e/{command_execution, variable_and_expansion, ...}/`
  directories or backfilling them with `POSIX_REF` lines.
- Refactoring / consolidating existing `e2e/posix_spec/2_*/`.
- Implementing missing builtins / features (`${var/pat/repl}`,
  function-local variables, etc.) — XFAIL tests serve as the
  acceptance spec.
- Harness extensions (`EXPECT_STDOUT_REGEX`, parallel runner,
  locale setup, etc.).
- Performance / benchmark tests.
- Non-XCU chapter coverage.
- CI workflow additions (macOS runner, fmt/clippy gates, etc.).

## 9. Next Steps

Upon spec approval:

1. Invoke `writing-plans` skill to produce the implementation plan
   covering all four phases as sub-phases within a single rolling
   plan.
2. Implementation begins at Phase 1 sub-phase 1.1 (2.1 + 2.2
   Quoting).
