# POSIX XCU Chapter 2 Coverage Matrix and Gap-Filling Tests — Design

- Date: 2026-04-18
- Author: Brainstorming session (yosh maintainer + Claude)
- Status: Approved, ready for implementation planning

## 1. Purpose and Success Criteria

### Purpose

Establish a systematic, chapter-by-chapter E2E verification of yosh's conformance to POSIX.1-2017 XCU Chapter 2 (Shell Command Language). The current E2E suite (257 tests) covers most of Chapter 2 but with uneven density and no single source that shows which sections are covered, thin, or missing.

### Success Criteria

1. Every subsection of XCU Chapter 2 (§2.1 through §2.14) has at least one representative E2E test. Informational or structural subsections (e.g., §2.1, §2.10) satisfy this with a minimal test demonstrating the concept is observable (e.g., for §2.4, a reserved word is interpreted as reserved in command position).
2. Subsections currently thin or missing dedicated coverage receive additional normative-requirement-based tests. Tentative targets: §2.3, §2.4, §2.5.3, §2.6.1, §2.8.1, §2.10, §2.11, §2.12.
3. New tests that fail on yosh because of real conformance gaps are marked with the existing `XFAIL:` metadata so `./e2e/run_tests.sh` continues to exit 0 (PASS / XFAIL only; no FAIL / XPASS / timeout).
4. A new document `docs/posix/chapter2-coverage.md` enumerates every Chapter 2 subsection, links to the tests covering it (both pre-existing and newly added), records XFAIL reasons, and classifies each subsection as `covered` / `thin` / `missing` / `informational`. This matrix becomes the single source for future coverage-deepening work.

### Non-Goals (YAGNI)

- Chapter 4 (Utilities) coverage expansion. Tracked as a separate future task in `TODO.md`.
- One-test-per-normative-clause saturation across *all* of Chapter 2. This spec only applies clause-level coverage to the subsections classified as `thin` in §4; the rest keep representative coverage. Full saturation is tracked as a separate future task in `TODO.md`.
- Reorganizing the existing 257 tests. Existing tests stay where they are; only new tests live under `e2e/posix_spec/`.

## 2. Coverage Matrix Document

### Location

`docs/posix/chapter2-coverage.md`

### Role

- Visualize coverage by chapter/section.
- Reverse-index: section → test file paths.
- Track XFAIL tests and the normative requirements they gate on.
- Provide the template for future expansion (Chapter 4, normative-granularity pass).

### Structure

Top-level layout:

```markdown
# POSIX.1-2017 XCU Chapter 2 Coverage Matrix

Source: https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html

## 2.1 Shell Introduction
- Status: informational
- Tests: e2e/posix_spec/2_01_shell_introduction/*.sh

## 2.2 Quoting
### 2.2.1 Escape Character
- Status: covered
- Tests:
  - e2e/quoting/backslash_basic.sh
  - e2e/posix_spec/2_02_01_escape_character/*.sh (if added)
- Normative clauses covered: ...
...
```

### Per-section entry fields

1. Section number + title (match POSIX online table of contents exactly)
2. Status classification: `covered` / `thin` / `missing` / `informational`
3. Test file paths (existing + new). Globs permitted.
4. If any XFAIL tests exist in this section, a summary of the XFAIL reasons.

### Generation policy

- Initial version is hand-written while consulting the POSIX online spec.
- Section structure mirrors the POSIX ToC 1:1.
- Keep the option to auto-regenerate later from `# POSIX_REF:` headers, but not implemented now.
- Expected size: 400–700 lines.

## 3. Directory and Naming Scheme

### New top-level directory

`e2e/posix_spec/`

### Subdirectory naming

Pattern: `<section_number_with_underscores>_<slug>/`

Initial set (to be confirmed against POSIX ToC during Phase 1):

- `2_01_shell_introduction/`
- `2_02_01_escape_character/`
- `2_02_02_single_quotes/`
- `2_02_03_double_quotes/`
- `2_03_token_recognition/`
- `2_03_01_alias_substitution/`
- `2_04_reserved_words/`
- `2_05_01_positional_parameters/`
- `2_05_02_special_parameters/`
- `2_05_03_shell_variables/`
- `2_06_01_tilde_expansion/`
- `2_06_05_field_splitting/`
- `2_06_06_pathname_expansion/`
- `2_06_07_quote_removal/`
- `2_07_redirection/`
- `2_08_01_consequences_of_shell_errors/`
- `2_08_02_exit_status_for_commands/`
- `2_09_shell_commands/`
- `2_10_shell_grammar/`
- `2_11_signals_and_error_handling/`
- `2_12_shell_execution_environment/`
- `2_13_pattern_matching/`
- `2_14_special_built_in_utilities/`

Rules:

- Dots in section numbers become underscores.
- Section numbers are zero-padded to two digits (e.g., `2_01`) so lexicographic sort matches chapter order.
- Subsections are flat directories (e.g., `2_02_01_...`, `2_02_02_...`), not nested under a `2_02_quoting/` parent.

### Test file naming

Existing convention: `<topic>.sh` (short, descriptive).

Examples:

- `e2e/posix_spec/2_04_reserved_words/case_keyword_in_command_position.sh`
- `e2e/posix_spec/2_10_shell_grammar/compound_list_newline_separator.sh`

### Metadata convention (unchanged from project norm)

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: <one-line description>
# EXPECT_OUTPUT: ...
# EXPECT_EXIT: 0
# XFAIL: <reason>   # only when the test currently fails on yosh
<script body>
```

- `POSIX_REF:` value is `<section_number> <section_title>`, matching the matrix doc section heading verbatim.
- File permissions must be `644` (project convention).

### Existing tests

Do not move. The matrix doc references them in place.

## 4. Per-Section Work Plan (provisional)

Provisional because final classification will be confirmed against the POSIX online spec in Phase 1.

| Section | Classification | Plan |
|---|---|---|
| 2.1 Shell Introduction | informational | 1 minimal observation test (script executes under `#!/bin/sh`) |
| 2.2 Quoting | covered | reference existing tests in matrix, no additions |
| 2.3 Token Recognition | thin | 2–3 token-boundary tests |
| 2.3.1 Alias Substitution | covered | reference existing tests |
| 2.4 Reserved Words | missing (dedicated) | 2–4 tests covering reserved-word-in-command-position vs word-position |
| 2.5 Parameters and Variables | covered | reference existing tests |
| 2.5.1 Positional Parameters | covered | reference existing tests |
| 2.5.2 Special Parameters | covered | reference existing tests |
| 2.5.3 Shell Variables | thin | 3–5 tests on `PS1/PS2/PS4`, `IFS`, `PATH`, `HOME`, `PWD`, `OLDPWD`, `PPID`, `LINENO` default-and-override behavior |
| 2.6.1 Tilde Expansion | missing | 2–3 tests: `~`, `~user`, `PATH=~/bin` assignment-RHS case |
| 2.6.2 Parameter Expansion | covered | reference existing tests |
| 2.6.3 Command Substitution | covered | reference existing tests |
| 2.6.4 Arithmetic Expansion | covered | reference existing tests |
| 2.6.5 Field Splitting | covered | reference existing tests |
| 2.6.6 Pathname Expansion | covered | reference existing tests |
| 2.6.7 Quote Removal | covered | reference existing tests |
| 2.7 Redirection (§2.7.*) | covered | reference existing tests |
| 2.8.1 Consequences of Shell Errors | thin | 2–3 tests: special-builtin syntax error terminates shell, redirection error behavior, etc. |
| 2.8.2 Exit Status for Commands | covered | reference existing tests |
| 2.9 Shell Commands (§2.9.*) | covered | reference existing tests |
| 2.10 Shell Grammar | missing (dedicated) | 3–5 grammar-boundary tests: `;` / `&` / newline terminator equivalence, compound_list newline interleaving, empty-list prohibitions |
| 2.11 Signals and Error Handling | thin | 3–5 tests on `trap` for POSIX-mandated signals beyond `EXIT`, child signal inheritance, `trap -` reset semantics |
| 2.12 Shell Execution Environment | thin | 2–4 tests: subshell env-var / cwd / open-fd inheritance and isolation |
| 2.13 Pattern Matching | covered | reference existing tests |
| 2.13.3 Patterns for Filename Expansion | covered | reference existing tests |
| 2.14 Special Built-In Utilities (§2.14.1..§2.14.N) | covered | enumerate per-builtin coverage in matrix; add minimal test only when a specific special builtin has none |

### Estimated new test count

Approximately 30–50 files across `e2e/posix_spec/` subdirectories.

### XFAIL policy

- After each new test is written, run it against the current yosh binary (`cargo build` then `./e2e/run_tests.sh --filter=<path>`).
- If it passes, leave `XFAIL:` absent.
- If it fails due to a genuine conformance gap, add `# XFAIL: <one-line reason>` describing **what** is not spec-compliant. Example: `# XFAIL: tilde expansion in PATH= assignment not implemented`.
- For every XFAIL added, append a matching item to `TODO.md` under a new or existing POSIX-conformance section so the gap is tracked as future work.
- If a test reveals behavior that is ambiguous in the spec, or where yosh's deviation is arguably defensible, do NOT commit the test. Instead record the question in the matrix doc and add a discussion-class item to `TODO.md`.

## 5. Execution Phases

Implementation details (file-by-file steps) live in the implementation plan produced by the writing-plans skill. This section fixes the phase boundaries.

### Phase 1: Build coverage matrix

- Fetch POSIX.1-2017 XCU Chapter 2 sections via WebFetch (`https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html`).
- Grep existing tests for `POSIX_REF:` values.
- Hand-author `docs/posix/chapter2-coverage.md` with every subsection, current coverage status, and references to existing test files.
- Commit: `docs(posix): add XCU Chapter 2 coverage matrix`
- **Deliverable:** matrix doc only. No test changes.

### Phase 2: Add representative tests for missing sections

- Target: §2.1, §2.4, §2.6.1, §2.10 (and any others revealed as missing in Phase 1).
- Create `e2e/posix_spec/<section>/` directories and add 1–5 tests each.
- Build and run each new test; apply XFAIL when appropriate.
- Update matrix doc (coverage status → `covered`, link new tests).
- Commit: `test(posix): cover missing Chapter 2 sections`
- **Deliverable:** new test files + matrix doc update.

### Phase 3: Deepen thin sections with normative tests

- Target: §2.3, §2.5.3, §2.8.1, §2.11, §2.12 (and any others revealed as thin in Phase 1).
- For each thin section, enumerate the **shall** clauses from the POSIX spec (skip `should` / `may` for this pass) and add one test per clause.
- Apply XFAIL as needed. Update matrix doc.
- Commit: `test(posix): deepen thin Chapter 2 sections`
- **Deliverable:** new test files + matrix doc update.

### Phase 4: Final reconciliation

- Run full suite: `cargo build && ./e2e/run_tests.sh`. Require exit 0 (PASS + XFAIL only; zero FAIL / XPASS / timeout).
- For every XFAIL added, ensure `TODO.md` has a matching future-work item.
- Re-tally matrix doc statistics (counts of covered / thin / missing / informational).
- Commit: `docs(posix): finalize Chapter 2 coverage matrix`
- **Deliverable:** all updates committed, `./e2e/run_tests.sh` green.

### Commit conventions

- One commit per phase minimum; split further when a phase grows too large for a single review.
- Prefixes: `docs(posix)` / `test(posix)` per existing project style.
- Commit body must include the original prompt context: this work traces back to a request to pursue full POSIX conformance via chapter-by-chapter E2E coverage.

### Verification commands

```sh
cargo build
./e2e/run_tests.sh --filter=posix_spec/   # new tests only
./e2e/run_tests.sh                        # full regression
```

### Handling ambiguous cases

If a test-under-construction surfaces behavior where yosh's deviation may be defensible (spec ambiguity, intentional non-POSIX extension, etc.), do not commit the test. Record the question in `docs/posix/chapter2-coverage.md` (in a dedicated "Open Questions" appendix) and add a discussion item to `TODO.md`.
