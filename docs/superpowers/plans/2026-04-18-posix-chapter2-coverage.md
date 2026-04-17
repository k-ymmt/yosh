# POSIX XCU Chapter 2 Coverage Matrix and Gap-Filling Tests — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a single coverage matrix for POSIX.1-2017 XCU Chapter 2 and fill gaps in the E2E suite so every subsection of Chapter 2 has at least representative coverage, with `XFAIL` used to register genuine conformance gaps.

**Architecture:** One living reference document (`docs/posix/chapter2-coverage.md`) enumerates every Chapter 2 subsection with classification and test links. New tests go under a new tree `e2e/posix_spec/<section>/` named after POSIX section numbers. Existing tests stay in place; the matrix doc references them by path. XFAIL failures get matching entries in `TODO.md`.

**Tech Stack:** Existing POSIX E2E runner (`e2e/run_tests.sh`), sh-based test scripts with metadata headers (`POSIX_REF`, `DESCRIPTION`, `EXPECT_OUTPUT`, `EXPECT_EXIT`, `EXPECT_STDERR`, `XFAIL`). Spec source: `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html` via WebFetch.

**Spec:** `docs/superpowers/specs/2026-04-18-posix-chapter2-coverage-design.md`

---

## Pre-flight Notes for Executor

1. **Build once up front:** `cargo build` before starting; rebuild only if `src/` changes.
2. **Test file permissions:** New test files must be `644`. Set with `chmod 644 <file>` after creation.
3. **Running a single new test:**
   ```sh
   ./e2e/run_tests.sh --filter=posix_spec/<subdir>/ --verbose
   ```
4. **XFAIL decision rule:**
   - If the test passes on yosh → no `XFAIL:` line, leave as-is.
   - If the test fails because yosh behavior differs from POSIX → add `# XFAIL: <short reason>` on the metadata block, rerun, confirm `[XFAIL]` in the output.
   - If the test fails due to a test-script bug (wrong expectation, typo) → fix the test, do not XFAIL.
5. **TODO.md updates:** Every `XFAIL` added must have a matching entry in `TODO.md` under a new section `## Future: POSIX Conformance Gaps (Chapter 2)`. Entry format: `- [ ] <short description> — <test path>:<xfail reason>`.
6. **Matrix doc updates:** After each task that adds or discovers tests, update the affected section in `docs/posix/chapter2-coverage.md` in the same commit.
7. **Commit per task** unless a task explicitly says otherwise.
8. **Spec verification:** Each "add tests" task starts with a WebFetch of the relevant POSIX online section to confirm the behavior being tested. Record the fetched URL in the commit message body.

---

## File Structure

**Created:**
- `docs/posix/chapter2-coverage.md` — coverage matrix doc (Task 1, 2, 12)
- `e2e/posix_spec/2_03_token_recognition/*.sh` — Task 4
- `e2e/posix_spec/2_04_reserved_words/*.sh` — Task 5
- `e2e/posix_spec/2_06_01_tilde_expansion/*.sh` — Task 6
- `e2e/posix_spec/2_08_01_consequences_of_shell_errors/*.sh` — Task 7
- `e2e/posix_spec/2_10_shell_grammar/*.sh` — Task 8
- `e2e/posix_spec/2_11_signals_and_error_handling/*.sh` — Task 9
- `e2e/posix_spec/2_13_pattern_matching/*.sh` — Task 10
- `e2e/posix_spec/2_05_03_shell_variables/*.sh` — Task 11

**Modified:**
- `TODO.md` — add POSIX Conformance Gaps section, record XFAIL items (throughout)
- `docs/posix/chapter2-coverage.md` — updated at every task boundary

**Not modified:** existing 257 tests remain in place.

---

## Task 1: Create Coverage Matrix Skeleton

**Files:**
- Create: `docs/posix/chapter2-coverage.md`

- [ ] **Step 1: Fetch POSIX Chapter 2 ToC**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html` with prompt `Extract the numbered section hierarchy (e.g., 2.1, 2.2.1, 2.2.2, ...) for XCU Chapter 2, with each section's title. Output as a flat list.`

Expected: complete section list from §2.1 through §2.14.x.

- [ ] **Step 2: Create skeleton matrix doc**

Create `docs/posix/chapter2-coverage.md` with the following structure (fill section titles from Step 1 output):

```markdown
# POSIX.1-2017 XCU Chapter 2 Coverage Matrix

Source: https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html

**Legend:**
- `covered` — at least one focused test exists
- `thin` — has tests, but fewer than 3 or missing major sub-behaviors
- `missing` — no dedicated test file
- `informational` — descriptive/structural section; minimal observation test is enough

## 2.1 Shell Introduction
- Status: TBD
- Tests: TBD

## 2.2 Quoting
- Status: TBD
- Tests: TBD

### 2.2.1 Escape Character
- Status: TBD
- Tests: TBD

...
```

(Continue for every section/subsection returned in Step 1.)

- [ ] **Step 3: Commit skeleton**

```sh
git add docs/posix/chapter2-coverage.md
git commit -m "$(cat <<'EOF'
docs(posix): skeleton Chapter 2 coverage matrix

Empty matrix structured by POSIX.1-2017 XCU Chapter 2 sections,
sourced from https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html.
Classification fields (Status/Tests) to be populated in the next task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Populate Matrix from Existing Tests

**Files:**
- Modify: `docs/posix/chapter2-coverage.md`

- [ ] **Step 1: Collect existing POSIX_REF → test path mapping**

Run:
```sh
grep -r '^# POSIX_REF:' e2e/ | awk -F':# POSIX_REF: ' '{print $2 "\t" $1}' | sort
```

Expected: lines of the form `<SECTION>\t<TEST_PATH>`. Preserve this output for reference.

- [ ] **Step 2: Populate matrix entries**

For every section in the matrix doc:
1. Set `Tests:` to the list of matching `e2e/...` paths (use globs when a directory has many entries; spell out paths when ≤3 files).
2. Set `Status:` by the rule:
   - 0 tests → `missing` (exception: §2.1, §2.10 and other purely descriptive sections → `informational` if the structural concept is exercised elsewhere; otherwise still `missing`)
   - 1–2 tests and the section has >2 major sub-behaviors → `thin`
   - ≥3 tests covering the major sub-behaviors → `covered`
3. For sections with existing tests using an inconsistent `POSIX_REF` label (e.g., tests labeled `2.5.3 Shell Execution Environment` that should be `2.5.3 Shell Variables`, or `2.11 Job Control` that should be `2.11 Signals and Error Handling`), **list them under their correct POSIX section** in the matrix but add a note: `Note: POSIX_REF header in these files uses legacy label "<old>"; fix scheduled in Task 11.`

- [ ] **Step 3: Add bottom-of-doc summary table**

Append to `docs/posix/chapter2-coverage.md`:

```markdown
## Summary

| Status | Count |
|---|---|
| covered | <N> |
| thin | <N> |
| missing | <N> |
| informational | <N> |

| Section | Status |
|---|---|
| 2.1 | ... |
| 2.2 | ... |
| ... | ... |
```

Fill counts and the per-section status table.

- [ ] **Step 4: Sanity-check classification**

Based on the existing `POSIX_REF` distribution (from `grep` in Step 1), verify these expected classifications are in the populated matrix:
- `missing`: §2.3 (Token Recognition — excluding 2.3.1), §2.4, §2.6.1, §2.8.1, §2.10
- `thin`: §2.11 Signals and Error Handling, §2.13 Pattern Matching, §2.5.3 Shell Variables
- `covered`: §2.2.*, §2.5.1, §2.5.2, §2.6.2–2.6.7, §2.7.*, §2.8.2, §2.9.*, §2.12, §2.14

If the matrix diverges from this, either:
- the existing test distribution disagrees → note the delta in the matrix under an "Open Questions" appendix and keep going, or
- the rule in Step 2 was misapplied → correct the matrix before committing.

- [ ] **Step 5: Commit populated matrix**

```sh
git add docs/posix/chapter2-coverage.md
git commit -m "$(cat <<'EOF'
docs(posix): populate Chapter 2 coverage matrix from existing tests

Every XCU Chapter 2 section classified as covered/thin/missing/informational
based on current e2e/ test distribution. Legacy POSIX_REF labels noted for
later correction in the 2.5.3 cleanup task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: §2.1 Shell Introduction — Verify Minimal Coverage

**Files:**
- Create (only if no §2.1 test qualifies): `e2e/posix_spec/2_01_shell_introduction/minimal_sh_script.sh`
- Modify: `docs/posix/chapter2-coverage.md`

- [ ] **Step 1: Check whether §2.1 already has a usable test**

Run:
```sh
grep -l '^# POSIX_REF: 2\.1 Shell Introduction' e2e/**/*.sh 2>/dev/null
```

If one match exists and its contents exercise that a `#!/bin/sh` script executes under yosh, **skip to Step 4** (mark §2.1 as `covered` in matrix).

- [ ] **Step 2: Write the minimal §2.1 test**

Create `e2e/posix_spec/2_01_shell_introduction/minimal_sh_script.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: Minimal POSIX sh script executes successfully
# EXPECT_OUTPUT: posix shell
# EXPECT_EXIT: 0
echo posix shell
```

Then: `chmod 644 e2e/posix_spec/2_01_shell_introduction/minimal_sh_script.sh`

- [ ] **Step 3: Run the new test**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_01_shell_introduction/
```

Expected: `[PASS]`. If it fails, yosh has a fundamental script-execution issue — stop and debug, do not XFAIL.

- [ ] **Step 4: Update matrix**

Set §2.1 status to `covered` in `docs/posix/chapter2-coverage.md` and list the test file path.

- [ ] **Step 5: Commit**

```sh
git add e2e/posix_spec/2_01_shell_introduction/ docs/posix/chapter2-coverage.md
git commit -m "$(cat <<'EOF'
test(posix): add minimal §2.1 Shell Introduction smoke test

Ensures Chapter 2 coverage matrix has an explicit entry for §2.1 even
though the section is primarily informational.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: §2.3 Token Recognition — Missing Coverage

**Files:**
- Create: `e2e/posix_spec/2_03_token_recognition/operator_terminates_word.sh`
- Create: `e2e/posix_spec/2_03_token_recognition/line_continuation_in_word.sh`
- Create: `e2e/posix_spec/2_03_token_recognition/quoted_operator_not_token.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.3**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_03` with prompt `Summarize the tokenization rules in §2.3 Token Recognition in bullet form. Focus on (a) when an operator terminates a word, (b) line continuation via backslash-newline, (c) quoting disabling operator recognition.`

Record the summary in the task notes.

- [ ] **Step 2: Write test — operator terminates word**

Create `e2e/posix_spec/2_03_token_recognition/operator_terminates_word.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Pipe operator terminates the preceding word without whitespace
# EXPECT_OUTPUT: hallo
# EXPECT_EXIT: 0
echo hello|tr e a
```

`chmod 644` the file.

- [ ] **Step 3: Write test — line continuation**

Create `e2e/posix_spec/2_03_token_recognition/line_continuation_in_word.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Backslash-newline is removed before tokenization
# EXPECT_OUTPUT: helloworld
# EXPECT_EXIT: 0
echo hello\
world
```

`chmod 644` the file.

- [ ] **Step 4: Write test — quoted operator**

Create `e2e/posix_spec/2_03_token_recognition/quoted_operator_not_token.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Operator characters inside quotes do not start a new token
# EXPECT_OUTPUT: a|b
# EXPECT_EXIT: 0
echo 'a|b'
```

`chmod 644` the file.

- [ ] **Step 5: Run new tests**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_03_token_recognition/ --verbose
```

For each test: if `[PASS]`, continue. If `[FAIL]`, add `# XFAIL: <reason>` line to the failing test, rerun, confirm `[XFAIL]`.

Common XFAIL reasons to use if applicable:
- `backslash-newline line continuation not handled in lexer`
- `pipe operator does not terminate word without whitespace`

- [ ] **Step 6: Update matrix and TODO.md**

In `docs/posix/chapter2-coverage.md`: change §2.3 status to `covered` and list the three test files.

In `TODO.md`: if any XFAIL was added in Step 5, add entries under a new section `## Future: POSIX Conformance Gaps (Chapter 2)` in the format `- [ ] §2.3 <behavior> — e2e/posix_spec/2_03_token_recognition/<file>.sh`.

- [ ] **Step 7: Commit**

```sh
git add e2e/posix_spec/2_03_token_recognition/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): add §2.3 Token Recognition coverage

Three representative tests: operator-terminates-word, line-continuation,
quoted-operator-not-token. Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_03

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: §2.4 Reserved Words — Missing Coverage

**Files:**
- Create: `e2e/posix_spec/2_04_reserved_words/if_in_command_position.sh`
- Create: `e2e/posix_spec/2_04_reserved_words/if_as_argument.sh`
- Create: `e2e/posix_spec/2_04_reserved_words/quoted_if_not_reserved.sh`
- Create: `e2e/posix_spec/2_04_reserved_words/brace_group_in_command_position.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.4**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_04` with prompt `List the POSIX reserved words from §2.4 and summarize the rule for when a word is recognized as reserved (command position only, unquoted).`

Expected: the 16 POSIX reserved words (`!`, `{`, `}`, `case`, `do`, `done`, `elif`, `else`, `esac`, `fi`, `for`, `if`, `in`, `then`, `until`, `while`) and the position rule.

- [ ] **Step 2: Write test — `if` in command position**

Create `e2e/posix_spec/2_04_reserved_words/if_in_command_position.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: 'if' in command position is recognized as reserved
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 0
if true; then echo yes; fi
```

`chmod 644` the file.

- [ ] **Step 3: Write test — `if` as argument**

Create `e2e/posix_spec/2_04_reserved_words/if_as_argument.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved words in argument position are ordinary words
# EXPECT_OUTPUT: if for while
# EXPECT_EXIT: 0
echo if for while
```

`chmod 644` the file.

- [ ] **Step 4: Write test — quoted reserved word**

Create `e2e/posix_spec/2_04_reserved_words/quoted_if_not_reserved.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Quoting a reserved word in command position looks up as command, not reserved
# EXPECT_EXIT: 127
# EXPECT_STDERR: not found
'if' true
```

`chmod 644` the file. Note: POSIX specifies that a quoted reserved word is no longer reserved; yosh should attempt to execute it as a command and fail with 127.

- [ ] **Step 5: Write test — brace group**

Create `e2e/posix_spec/2_04_reserved_words/brace_group_in_command_position.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: '{' and '}' act as grouping reserved words in command position
# EXPECT_OUTPUT: grouped
# EXPECT_EXIT: 0
{ echo grouped; }
```

`chmod 644` the file.

- [ ] **Step 6: Run and XFAIL as needed**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_04_reserved_words/ --verbose
```

For failures, add `# XFAIL:` with the actual reason observed. Common possibilities:
- `yosh treats quoted reserved words as still-reserved`
- `brace group recognition not implemented in command position`

- [ ] **Step 7: Update matrix and TODO.md**

Matrix: §2.4 → `covered`, list 4 tests. TODO.md: add entries for any XFAIL.

- [ ] **Step 8: Commit**

```sh
git add e2e/posix_spec/2_04_reserved_words/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): add §2.4 Reserved Words coverage

Tests: if-in-command-position, if-as-argument, quoted-if-not-reserved,
brace-group-in-command-position. Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_04

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: §2.6.1 Tilde Expansion — Missing Coverage

**Files:**
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_home.sh`
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_slash_path.sh`
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`
- Create: `e2e/posix_spec/2_06_01_tilde_expansion/tilde_quoted_no_expansion.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.6.1**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_06_01` with prompt `Summarize the tilde-prefix expansion rules in §2.6.1 covering: unquoted leading tilde, tilde with user name, tilde on the right side of an unquoted '=' in a variable assignment, and when tilde is NOT expanded (quoted).`

- [ ] **Step 2: Write test — `~` alone**

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_home.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Unquoted '~' expands to $HOME
# EXPECT_OUTPUT: /tmp/hdir
# EXPECT_EXIT: 0
HOME=/tmp/hdir
echo ~
```

`chmod 644`.

- [ ] **Step 3: Write test — `~/path`**

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_slash_path.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: '~/path' expands to $HOME/path
# EXPECT_OUTPUT: /tmp/hdir/bin
# EXPECT_EXIT: 0
HOME=/tmp/hdir
echo ~/bin
```

`chmod 644`.

- [ ] **Step 4: Write test — assignment RHS**

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_assignment_rhs.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde following unquoted '=' in variable assignment expands
# EXPECT_OUTPUT: /tmp/hdir/bin
# EXPECT_EXIT: 0
HOME=/tmp/hdir
x=~/bin
echo "$x"
```

`chmod 644`.

- [ ] **Step 5: Write test — quoted tilde**

Create `e2e/posix_spec/2_06_01_tilde_expansion/tilde_quoted_no_expansion.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Quoted tilde is not expanded
# EXPECT_OUTPUT: ~
# EXPECT_EXIT: 0
echo '~'
```

`chmod 644`.

- [ ] **Step 6: Run and XFAIL as needed**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_06_01_tilde_expansion/ --verbose
```

Common XFAIL reasons:
- `tilde expansion on assignment RHS not implemented`
- `bare '~' not expanded when HOME is set`

- [ ] **Step 7: Update matrix and TODO.md**

Matrix: §2.6.1 → `covered`, list 4 tests. TODO.md: XFAIL entries.

- [ ] **Step 8: Commit**

```sh
git add e2e/posix_spec/2_06_01_tilde_expansion/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): add §2.6.1 Tilde Expansion coverage

Tests: bare ~, ~/path, assignment-RHS tilde, quoted tilde non-expansion.
Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_06_01

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: §2.8.1 Consequences of Shell Errors — Missing Coverage

**Files:**
- Create: `e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_syntax_error.sh`
- Create: `e2e/posix_spec/2_08_01_consequences_of_shell_errors/redirection_error_regular_command.sh`
- Create: `e2e/posix_spec/2_08_01_consequences_of_shell_errors/command_not_found_continues.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.8.1**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_08_01` with prompt `List the categories of shell errors in §2.8.1 and for each category, state whether a non-interactive shell exits or continues.`

Focus on: special-builtin errors (shall exit), redirection errors on simple commands (command fails, shell continues), command not found (command fails with 127, shell continues).

- [ ] **Step 2: Write test — special builtin error**

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_syntax_error.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: ':?' expansion error in a non-interactive shell terminates the shell
# EXPECT_EXIT: 1
# EXPECT_STDERR: required
unset FOO
: "${FOO:?required}"
echo "unreachable"
```

`chmod 644`. Rationale: POSIX §2.6.2 `${var:?word}` says that when the expansion error occurs in a non-interactive shell, the shell shall exit. §2.8.1 confirms the consequence. `echo unreachable` must not run. Exact exit status is implementation-defined but non-zero.

- [ ] **Step 3: Write test — redirection error on simple command**

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/redirection_error_regular_command.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Redirection error on a non-special-builtin command fails that command but does not exit the shell
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
cat </nonexistent/path/hopefully 2>/dev/null
echo after
```

`chmod 644`.

- [ ] **Step 4: Write test — command not found continues**

Create `e2e/posix_spec/2_08_01_consequences_of_shell_errors/command_not_found_continues.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: 'command not found' does not exit the shell
# EXPECT_OUTPUT: survived
# EXPECT_EXIT: 0
no_such_command_zzz 2>/dev/null
echo survived
```

`chmod 644`.

- [ ] **Step 5: Run and XFAIL as needed**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_08_01_consequences_of_shell_errors/ --verbose
```

Common XFAIL reason:
- `special builtin syntax error does not terminate non-interactive shell`

- [ ] **Step 6: Update matrix and TODO.md**

- [ ] **Step 7: Commit**

```sh
git add e2e/posix_spec/2_08_01_consequences_of_shell_errors/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): add §2.8.1 Consequences of Shell Errors coverage

Tests: special-builtin syntax error exits shell, redirection error on
regular command continues, command-not-found continues. Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_08_01

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: §2.10 Shell Grammar — Missing Coverage

**Files:**
- Create: `e2e/posix_spec/2_10_shell_grammar/terminator_semicolon_equals_newline.sh`
- Create: `e2e/posix_spec/2_10_shell_grammar/compound_list_newline_between_commands.sh`
- Create: `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.10**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_10` with prompt `From the §2.10 Shell Grammar BNF, summarize: (1) equivalence of ';', '&', and newline as list terminators; (2) that compound_list allows arbitrary newlines between commands; (3) that compound_list is non-empty.`

- [ ] **Step 2: Write test — `;` vs newline terminator**

Create `e2e/posix_spec/2_10_shell_grammar/terminator_semicolon_equals_newline.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: ';' and newline are interchangeable as list terminators
# EXPECT_OUTPUT<<END
# one
# two
# three
# END
# EXPECT_EXIT: 0
echo one; echo two
echo three
```

`chmod 644`.

- [ ] **Step 3: Write test — compound_list newline interleaving**

Create `e2e/posix_spec/2_10_shell_grammar/compound_list_newline_between_commands.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: compound_list accepts newlines between commands inside if/then/fi
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
if true
then
    echo a
    echo b
fi
```

`chmod 644`.

- [ ] **Step 4: Write test — empty compound_list is syntax error**

Create `e2e/posix_spec/2_10_shell_grammar/empty_compound_list_in_if_is_error.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty compound_list inside 'if ... then fi' is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then
fi
```

`chmod 644`. Note: exit code 2 is yosh's syntax-error convention per CLAUDE.md.

- [ ] **Step 5: Run and XFAIL**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_10_shell_grammar/ --verbose
```

- [ ] **Step 6: Update matrix and TODO.md**

- [ ] **Step 7: Commit**

```sh
git add e2e/posix_spec/2_10_shell_grammar/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): add §2.10 Shell Grammar coverage

Tests: list terminator equivalence, compound_list newline interleaving,
empty compound_list rejected. Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_10

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: §2.11 Signals and Error Handling — Thin Section

**Files:**
- Create: `e2e/posix_spec/2_11_signals_and_error_handling/trap_exit_runs_on_exit.sh`
- Create: `e2e/posix_spec/2_11_signals_and_error_handling/trap_dash_resets_default.sh`
- Create: `e2e/posix_spec/2_11_signals_and_error_handling/trap_ignored_signal_inherited.sh`
- Create: `e2e/posix_spec/2_11_signals_and_error_handling/trap_int_by_name.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.11**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_11` with prompt `List the shall-clauses of §2.11 Signals and Error Handling: what must trap support (EXIT, signal names/numbers, '-' to reset, ''/empty to ignore), and how signals ignored at shell entry are treated.`

- [ ] **Step 2: Write test — trap EXIT**

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_exit_runs_on_exit.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap on EXIT runs when the shell exits
# EXPECT_OUTPUT<<END
# before
# on_exit
# END
# EXPECT_EXIT: 0
trap 'echo on_exit' EXIT
echo before
```

`chmod 644`.

- [ ] **Step 3: Write test — trap -**

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_dash_resets_default.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: 'trap - SIGNAL' resets the trap to default disposition
# EXPECT_OUTPUT<<END
# set
# reset
# END
# EXPECT_EXIT: 0
trap 'echo traphit' INT
echo set
trap - INT
echo reset
```

`chmod 644`.

- [ ] **Step 4: Write test — signal ignored at entry stays ignored**

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_ignored_signal_inherited.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Signals ignored on shell entry remain ignored even after 'trap ... SIGNAL'
# EXPECT_OUTPUT: still_alive
# EXPECT_EXIT: 0
# Launch a subshell with SIGINT ignored, then try to trap it.
sh -c 'trap "" INT; exec sh -c "trap \"echo trapped\" INT; kill -INT \$\$; echo still_alive"'
```

`chmod 644`. Note: this is an XFAIL candidate; the inheritance semantics are tricky.

- [ ] **Step 5: Write test — trap INT by name**

Create `e2e/posix_spec/2_11_signals_and_error_handling/trap_int_by_name.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap can reference signals by their POSIX name (INT)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo caught' INT
echo ok
```

`chmod 644`.

- [ ] **Step 6: Run and XFAIL as needed**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_11_signals_and_error_handling/ --verbose
```

If `trap_ignored_signal_inherited.sh` is brittle (kill delivery race), accept XFAIL with reason `ignored-signal-inherited semantics test is race-prone; investigate with timed delivery`.

- [ ] **Step 7: Update matrix and TODO.md**

Matrix: §2.11 → `covered`.

- [ ] **Step 8: Commit**

```sh
git add e2e/posix_spec/2_11_signals_and_error_handling/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): deepen §2.11 Signals and Error Handling coverage

Tests: trap EXIT, trap - reset, ignored-on-entry inheritance, trap by signal name.
Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_11

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: §2.13 Pattern Matching — Thin Section

**Files:**
- Create: `e2e/posix_spec/2_13_pattern_matching/star_matches_any_string.sh`
- Create: `e2e/posix_spec/2_13_pattern_matching/question_matches_single_char.sh`
- Create: `e2e/posix_spec/2_13_pattern_matching/bracket_char_class.sh`
- Create: `e2e/posix_spec/2_13_pattern_matching/bracket_negated_class.sh`
- Create: `e2e/posix_spec/2_13_pattern_matching/quoted_glob_literal.sh`
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.13**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_13` with prompt `Summarize the pattern-matching notation shall-clauses in §2.13: '*', '?', '[...]' character classes, negation '[!...]', and literal matching when metachars are quoted.`

- [ ] **Step 2: Write test — `*`**

Create `e2e/posix_spec/2_13_pattern_matching/star_matches_any_string.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: '*' in a case pattern matches any string including empty
# EXPECT_OUTPUT<<END
# caught empty
# caught hello
# END
# EXPECT_EXIT: 0
for arg in "" hello; do
    case "$arg" in
        *) echo "caught ${arg:-empty}" ;;
    esac
done
```

`chmod 644`.

- [ ] **Step 3: Write test — `?`**

Create `e2e/posix_spec/2_13_pattern_matching/question_matches_single_char.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: '?' matches exactly one character
# EXPECT_OUTPUT<<END
# one
# notone
# END
# EXPECT_EXIT: 0
case a in ?) echo one ;; *) echo notone ;; esac
case ab in ?) echo one ;; *) echo notone ;; esac
```

`chmod 644`.

- [ ] **Step 4: Write test — `[...]`**

Create `e2e/posix_spec/2_13_pattern_matching/bracket_char_class.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Bracket expression matches any contained char
# EXPECT_OUTPUT: match
# EXPECT_EXIT: 0
case b in [abc]) echo match ;; *) echo no ;; esac
```

`chmod 644`.

- [ ] **Step 5: Write test — `[!...]`**

Create `e2e/posix_spec/2_13_pattern_matching/bracket_negated_class.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: '[!...]' is a negated bracket expression
# EXPECT_OUTPUT: not_in
# EXPECT_EXIT: 0
case z in [!abc]) echo not_in ;; *) echo in ;; esac
```

`chmod 644`.

- [ ] **Step 6: Write test — quoted metachar is literal**

Create `e2e/posix_spec/2_13_pattern_matching/quoted_glob_literal.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: A quoted '*' in a case pattern matches only a literal asterisk
# EXPECT_OUTPUT: literal_star
# EXPECT_EXIT: 0
case '*' in '*') echo literal_star ;; *) echo glob ;; esac
```

`chmod 644`.

- [ ] **Step 7: Run and XFAIL**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_13_pattern_matching/ --verbose
```

- [ ] **Step 8: Update matrix and TODO.md**

- [ ] **Step 9: Commit**

```sh
git add e2e/posix_spec/2_13_pattern_matching/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): deepen §2.13 Pattern Matching Notation coverage

Tests: '*', '?', '[...]', '[!...]', and quoted-literal metachar. Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_13

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: §2.5.3 Shell Variables — Thin Section + Legacy Label Cleanup

**Files:**
- Create: `e2e/posix_spec/2_05_03_shell_variables/ifs_default_whitespace.sh`
- Create: `e2e/posix_spec/2_05_03_shell_variables/ifs_custom_splitting.sh`
- Create: `e2e/posix_spec/2_05_03_shell_variables/home_default_and_override.sh`
- Create: `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`
- Create: `e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh`
- Modify: existing test files whose `POSIX_REF` header mislabels §2.5.3 as "Shell Execution Environment" (discovered in Task 2)
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Fetch spec for §2.5.3**

Run: WebFetch `https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_05_03` with prompt `List each shell variable §2.5.3 mandates shall-support (HOME, IFS, LANG, LC_*, LINENO, NLSPATH, PATH, PPID, PS1, PS2, PS4, PWD), noting for each what behavior the shell shall provide.`

- [ ] **Step 2: Fix legacy POSIX_REF labels**

Locate files that use the legacy `POSIX_REF: 2.5.3 Shell Execution Environment` header:

```sh
grep -l '^# POSIX_REF: 2\.5\.3 Shell Execution Environment' e2e/**/*.sh 2>/dev/null
```

In each such file, replace that line with `# POSIX_REF: 2.5.3 Shell Variables`.

If the grep in Task 2 Step 1 also revealed `POSIX_REF: 2.11 Job Control` entries that should have been `2.11 Signals and Error Handling` (or a separate label the matrix decided to use for job-control subsection), do **not** re-label them in this task — the 2.11 subsection classification was already settled in Task 9. If there is a remaining mismatch, record it in the matrix "Open Questions" appendix and move on.

- [ ] **Step 3: Write test — IFS default**

Create `e2e/posix_spec/2_05_03_shell_variables/ifs_default_whitespace.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: Default IFS splits on space, tab, and newline
# EXPECT_OUTPUT<<END
# 3
# END
# EXPECT_EXIT: 0
set -- $(printf 'a\tb c')
echo $#
```

`chmod 644`.

- [ ] **Step 4: Write test — IFS custom**

Create `e2e/posix_spec/2_05_03_shell_variables/ifs_custom_splitting.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: IFS=':' splits on colon only
# EXPECT_OUTPUT<<END
# a|b|c
# END
# EXPECT_EXIT: 0
IFS=:
set -- $(printf 'a:b:c')
IFS=' '
echo "$*" | tr ' ' '|'
```

`chmod 644`.

- [ ] **Step 5: Write test — HOME default/override**

Create `e2e/posix_spec/2_05_03_shell_variables/home_default_and_override.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: HOME can be overridden and read back
# EXPECT_OUTPUT: /tmp/h
# EXPECT_EXIT: 0
HOME=/tmp/h
echo "$HOME"
```

`chmod 644`.

- [ ] **Step 6: Write test — PWD after cd**

Create `e2e/posix_spec/2_05_03_shell_variables/pwd_after_cd.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: PWD reflects the current working directory after cd
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
echo "$PWD"
```

`chmod 644`.

- [ ] **Step 7: Write test — LINENO**

Create `e2e/posix_spec/2_05_03_shell_variables/lineno_in_script.sh`:

```sh
#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO expands to the current script line number
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
echo $LINENO
```

`chmod 644`. Note: the `echo` is on line 6 (lines 1–5 are shebang + metadata comments). If yosh numbers LINENO starting from a different origin (e.g., counts only non-comment lines), adjust the expected value or XFAIL with that reason.

- [ ] **Step 8: Run and XFAIL**

Run:
```sh
./e2e/run_tests.sh --filter=posix_spec/2_05_03_shell_variables/ --verbose
```

Also rerun the legacy-relabeled tests:

```sh
./e2e/run_tests.sh --filter=<path of each relabeled test>
```

Expected: all relabeled tests still PASS (label change is header-only).

- [ ] **Step 9: Update matrix and TODO.md**

Matrix: §2.5.3 → `covered`, list new + relabeled tests. Remove the "Note: POSIX_REF header in these files uses legacy label" line now that it's cleaned up.

- [ ] **Step 10: Commit**

```sh
git add e2e/posix_spec/2_05_03_shell_variables/ e2e/ docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
test(posix): deepen §2.5.3 Shell Variables coverage and fix legacy labels

Five new tests (IFS default, IFS custom, HOME, PWD, LINENO) plus
POSIX_REF header normalization for existing files mislabeled as
"2.5.3 Shell Execution Environment" (correct title: "Shell Variables").
Spec reference:
https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html#tag_18_05_03

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Final Reconciliation

**Files:**
- Modify: `docs/posix/chapter2-coverage.md`, `TODO.md`

- [ ] **Step 1: Full regression run**

Run:
```sh
cargo build
./e2e/run_tests.sh
```

Expected output tail:
```
Total: <N>  Passed: <N>  Failed: 0  Timedout: 0  XFail: <N>  XPass: 0
```

Hard requirement: `Failed: 0`, `Timedout: 0`, `XPass: 0`. If any are non-zero, stop and fix before proceeding:
- `Failed` → either the test expectation is wrong (fix it) or the gap requires `XFAIL` (add it).
- `Timedout` → the test is pathological or has an infinite loop; fix.
- `XPass` → yosh now passes a test that was marked XFAIL; remove the `XFAIL:` line and the matching `TODO.md` entry.

- [ ] **Step 2: Re-tally matrix summary**

Update the summary table at the bottom of `docs/posix/chapter2-coverage.md`:

```sh
# Count existing tests per section after all additions
grep -r '^# POSIX_REF:' e2e/ | awk -F'# POSIX_REF: ' '{print $2}' | sort | uniq -c
```

Update the `| Status | Count |` and per-section tables.

- [ ] **Step 3: Verify every XFAIL has a TODO.md entry**

Run:
```sh
grep -r '^# XFAIL:' e2e/posix_spec/ | awk -F':# XFAIL: ' '{print $1 ": " $2}'
```

For each line, confirm `TODO.md` has a matching entry under `## Future: POSIX Conformance Gaps (Chapter 2)`. Add any missing.

- [ ] **Step 4: Verify no informational or missing classifications remain**

In `docs/posix/chapter2-coverage.md`, confirm:
- `missing`: 0 (all previously-missing sections now have tests)
- `informational`: at most §2.1 and §2.10 (or 0 if both ended up with representative tests)
- No subsection without a `Tests:` value

- [ ] **Step 5: Commit**

```sh
git add docs/posix/chapter2-coverage.md TODO.md
git commit -m "$(cat <<'EOF'
docs(posix): finalize Chapter 2 coverage matrix

All Chapter 2 subsections have representative E2E coverage. XFAIL entries
track genuine conformance gaps in TODO.md "POSIX Conformance Gaps (Chapter 2)".
./e2e/run_tests.sh exits 0 with PASS + XFAIL only.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review Notes (for the plan author, not the executor)

1. **Spec coverage:** Every spec section maps to tasks —
   - Spec §1 Purpose → Tasks 1-12 (all contribute to the success criteria)
   - Spec §2 Matrix doc → Tasks 1, 2, 12
   - Spec §3 Directory scheme → followed in Tasks 3-11
   - Spec §4 Per-section work plan → Tasks 3 (§2.1), 4 (§2.3), 5 (§2.4), 6 (§2.6.1), 7 (§2.8.1), 8 (§2.10), 9 (§2.11), 10 (§2.13), 11 (§2.5.3). Note: §2.12 was classified "thin" in the spec but existing grep shows 11 tests — reclassify as `covered` during Task 2; no dedicated task needed.
   - Spec §5 Execution phases → Phase 1 = Task 1+2, Phase 2 = Tasks 3-8, Phase 3 = Tasks 9-11, Phase 4 = Task 12
2. **XFAIL / TODO.md contract:** Every test-adding task has Step N "Update matrix and TODO.md" making this explicit.
3. **Naming consistency:** Directory names use `2_NN_NN_slug` (zero-padded). Verified across all tasks.
4. **No speculative reclassifications:** The "thin"/"missing" classification in Task 2 Step 4 lists the expected classifications as a sanity check, not a directive — if the actual test distribution differs, the matrix follows the data and this plan's later tasks remain valid (they simply may find fewer gaps than anticipated).
