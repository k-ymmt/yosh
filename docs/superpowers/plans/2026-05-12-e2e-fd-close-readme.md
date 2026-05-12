# E2E Test Expansion Cleanup — fd_close + README Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drain three of the five `Future: E2E Test Expansion` TODO items: strengthen `fd_close.sh` to verify the actual fd-close effect, document the `4 Utilities - <name>` POSIX_REF form in `e2e/README.md`, and record that the `$0` divergence is not actionable.

**Architecture:** Three independent text-edit changes (one shell test, one README, one TODO.md). No Rust code touched. Verification is via the existing `./e2e/run_tests.sh` harness; commit is single, on `main` per project memory.

**Tech Stack:** POSIX shell (`/bin/sh`-compatible test scripts), the yosh e2e harness (`e2e/run_tests.sh`), Cargo for sanity test run.

**Spec:** `docs/superpowers/specs/2026-05-12-e2e-fd-close-readme-design.md`

---

## File Map

- **Modify:** `e2e/redirection/fd_close.sh` — replace 4-line script with strengthened version that includes `EXPECT_STDERR: to stderr`.
- **Modify:** `e2e/README.md` — add a fifth bullet to the POSIX_REF Format Contract list (L94–101 region), and change the sample grep root at L107 from `e2e/posix_spec/` to `e2e/`.
- **Modify:** `TODO.md` — delete the L112 (`fd_close.sh`), L115 (`$0` divergence), L116 (README contract) entries under `Future: E2E Test Expansion`.

No new files. No code paths affected.

---

### Task 1: Strengthen `fd_close.sh` assertion

**Files:**
- Modify: `e2e/redirection/fd_close.sh` (entire file)

**Rationale:** The current 4-line test only checks `EXPECT_EXIT: 0` and does not verify that `>&2 2>&-` preserves the dup'd target on fd 1. Adding a substring `EXPECT_STDERR` assertion catches any future regression where redirection ordering or dup-then-close semantics break.

- [ ] **Step 1: Read the current file**

Run:
```bash
cat e2e/redirection/fd_close.sh
```

Expected output (current state):
```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: File descriptor close with N>&-
# EXPECT_EXIT: 0
echo "to stderr" >&2 2>&-
```

- [ ] **Step 2: Replace the file contents**

Replace the file with exactly:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: Per-command "2>&-" after ">&2" closes fd 2; dup'd target on fd 1 survives
# EXPECT_STDERR: to stderr
# EXPECT_EXIT: 0
echo "to stderr" >&2 2>&-
```

Changes vs. current:
1. `DESCRIPTION` line is rewritten to state the actual semantic under test.
2. New line: `# EXPECT_STDERR: to stderr` inserted between DESCRIPTION and EXPECT_EXIT.
3. The shell command itself is unchanged.

Note: `EXPECT_OUTPUT:` is **deliberately not added**. The harness parser at `e2e/run_tests.sh:223` matches `"# EXPECT_OUTPUT: "*` requiring a trailing space; the no-trailing-space form silently disables the stdout check rather than asserting empty stdout. Avoiding the misleading line until that quirk is fixed elsewhere.

- [ ] **Step 3: Verify the file mode is preserved at 644**

Run:
```bash
stat -f '%Sp' e2e/redirection/fd_close.sh
```

Expected: `-rw-r--r--` (per project convention: E2E test files should have `644` permissions).

If not 644, fix:
```bash
chmod 644 e2e/redirection/fd_close.sh
```

- [ ] **Step 4: Run the filtered test**

Run:
```bash
./e2e/run_tests.sh --filter=fd_close
```

Expected output (final lines):
```
[PASS]  redirection/fd_close.sh

── Summary ──
Total: 1  Passed: 1  Failed: 0  Timedout: 0  XFail: 0  XPass: 0
```

If FAIL: inspect with `./e2e/run_tests.sh --filter=fd_close -v` and check that yosh's stderr output contains the substring `to stderr`. A passing run confirms the new `EXPECT_STDERR` assertion is active and matches.

---

### Task 2: Update README POSIX_REF Format Contract

**Files:**
- Modify: `e2e/README.md` (L94–108 region)

**Rationale:** Chapter 4 utility tests (32 files matching `POSIX_REF: 4 Utilities`) use a form not listed in the contract. The sample grep also points at `e2e/posix_spec/` which excludes `e2e/builtin/` (where Chapter 4 tests live).

- [ ] **Step 1: Read the current contract section**

Run:
```bash
sed -n '89,108p' e2e/README.md
```

Expected output (current state):
```markdown
## POSIX_REF Format Contract

Test files declare which POSIX clause they pin via the `POSIX_REF`
metadata line. The accepted shapes are:

- `2.X.Y <Section Name>` — for ordinary section references.
  Example: `POSIX_REF: 2.6.1 Tilde Expansion`
- `2.10.2 Rule N - <Name>` — for tests that pin a specific grammar rule.
  Example: `POSIX_REF: 2.10.2 Rule 1 - First Word`
- `2.10.2 Rule N - <Name> (<discriminator>)` — for multi-case rules.
  Example: `POSIX_REF: 2.10.2 Rule 10 - Keyword (after pipe)`
- `2.10 Shell Grammar - <Topic>` — for cross-rule grammar topics.
  Example: `POSIX_REF: 2.10 Shell Grammar - Terminator Equality`

A naive grep for one shape misses the others. To enumerate all
§2.10.2-related tests, use:

` ` `sh
grep -RE 'POSIX_REF: 2\.10' e2e/posix_spec/
` ` `
```

- [ ] **Step 2: Add Chapter 4 bullet after the `2.10 Shell Grammar - <Topic>` entry**

Using the Edit tool, change this exact block:
```markdown
- `2.10 Shell Grammar - <Topic>` — for cross-rule grammar topics.
  Example: `POSIX_REF: 2.10 Shell Grammar - Terminator Equality`

A naive grep for one shape misses the others. To enumerate all
```

To:
```markdown
- `2.10 Shell Grammar - <Topic>` — for cross-rule grammar topics.
  Example: `POSIX_REF: 2.10 Shell Grammar - Terminator Equality`
- `4 Utilities - <name>` — for Chapter 4 utility tests (XCU Chapter 4).
  Example: `POSIX_REF: 4 Utilities - test`

A naive grep for one shape misses the others. To enumerate all
```

- [ ] **Step 3: Fix the sample grep search root**

Using the Edit tool, change this exact line:
```sh
grep -RE 'POSIX_REF: 2\.10' e2e/posix_spec/
```

To:
```sh
grep -RE 'POSIX_REF: 2\.10' e2e/
```

(Only the search root changes; the search pattern is unchanged.)

- [ ] **Step 4: Verify the edits**

Run:
```bash
sed -n '89,112p' e2e/README.md
```

Expected: the section now has five bullets ending with the new `4 Utilities - <name>` entry, and the grep example reads `grep -RE 'POSIX_REF: 2\.10' e2e/`.

- [ ] **Step 5: Verify the new grep example actually works**

Run:
```bash
grep -RE 'POSIX_REF: 2\.10' e2e/ | wc -l
```

Expected: a positive integer (currently around 100+ matches). Confirms the grep root is valid for the corpus.

---

### Task 3: Remove completed and non-actionable TODOs

**Files:**
- Modify: `TODO.md` (`Future: E2E Test Expansion` section)

**Rationale:** Per project convention (`CLAUDE.md`: "Delete completed items rather than marking them with `[x]`"), remove the three items now drained: L112 (completed in Task 1), L116 (completed in Task 2), and L115 (reported as not actionable — POSIX §2.5.2 permits both interpretations).

L113 and L114 remain — they are independent large projects.

- [ ] **Step 1: Read the section to locate exact line text**

Run:
```bash
grep -n -E 'fd_close\.sh|chapter-by-chapter POSIX|normative-requirement|\$0 divergence|POSIX_REF Format Contract' TODO.md
```

Expected: line numbers for the four E2E-section entries (one for fd_close, one for chapter coverage, one for normative depth, one for `$0`, one for README contract). Cross-check against the section header `## Future: E2E Test Expansion`.

- [ ] **Step 2: Delete the L112 `fd_close.sh` entry**

Using the Edit tool, remove this exact bullet (single line):
```markdown
- [ ] `fd_close.sh` test only checks exit code, not actual fd close effect
```

- [ ] **Step 3: Delete the `$0` divergence entry (current L115)**

Using the Edit tool, remove this exact bullet (multi-line). The block begins with `- [ ] yosh \`$0\` divergence` and ends with `Discovered 2026-05-12 during POSIX TODO cleanup batch.`. Match the entire bullet including all wrapped lines.

- [ ] **Step 4: Delete the README contract entry (current L116)**

Using the Edit tool, remove this exact bullet. The block begins with `- [ ] \`e2e/README.md\` POSIX_REF Format Contract is missing` and ends with `Surfaced during the 2026-05-12 E2E quick-wins final review.`.

- [ ] **Step 5: Verify the remaining section content**

Run:
```bash
awk '/^## Future: E2E Test Expansion/,/^## /' TODO.md | sed '$d'
```

Expected: header plus exactly two bullets remaining — L113 (`Extend chapter-by-chapter POSIX coverage beyond XCU Chapter 2`) and L114 (`Deepen Chapter 2 POSIX coverage to normative-requirement granularity`).

---

### Task 4: Regression verification

**Files:** (none modified)

**Rationale:** Confirm the changes do not break the rest of the e2e corpus or Rust tests. Per user global CLAUDE.md: "Ensure all tests pass before committing changes."

- [ ] **Step 1: Run the full e2e suite**

Run:
```bash
./e2e/run_tests.sh
```

Expected: completes with `Failed: 0` in the summary line. Note: the suite takes several minutes; the existing pass count should be preserved (no new FAILs introduced by the assertion strengthening). XFAIL count may shift only if a previously-failing test was somehow swept in, which is not the case here.

If any test newly FAILs, inspect with `-v` filter and revert; do not proceed to commit until clean.

- [ ] **Step 2: Run cargo test as a sanity check**

Run:
```bash
cargo test
```

Expected: all tests pass. Rust code is untouched in this plan; this guards against any unrelated transient regression in the workspace.

Note (from memory): `cargo test` without `-p`/`--workspace` may not find every yosh-related test — for this plan's purpose a clean run of the default-members suite is sufficient since no Rust files changed.

- [ ] **Step 3: Confirm only the three intended files differ**

Run:
```bash
git status --short
```

Expected exactly:
```
 M TODO.md
 M e2e/README.md
 M e2e/redirection/fd_close.sh
```

Any other modified or untracked files should be investigated before committing.

---

### Task 5: Commit

**Files:** (commit the three modifications above)

**Rationale:** Single commit on `main` per project memory. Commit message includes the original task context for traceability (per user global CLAUDE.md) and an explicit note that L115 was assessed as not-actionable.

- [ ] **Step 1: Stage the three files explicitly**

Run:
```bash
git add e2e/redirection/fd_close.sh e2e/README.md TODO.md
```

(Explicit file paths — per project policy, avoid `git add -A` / `git add .` to keep the staging area predictable.)

- [ ] **Step 2: Create the commit**

Run:
```bash
git commit -m "$(cat <<'EOF'
test(e2e): strengthen fd_close + document Chapter 4 POSIX_REF form

Drains three items from TODO.md "Future: E2E Test Expansion":

- L112: e2e/redirection/fd_close.sh now asserts EXPECT_STDERR: to stderr,
  verifying that ">&2 2>&-" preserves the dup'd target on fd 1 when fd 2
  is subsequently closed. Previously only EXPECT_EXIT was checked.
- L116: e2e/README.md POSIX_REF Format Contract gains a fifth bullet for
  "4 Utilities - <name>" (32 Chapter 4 tests already use this form), and
  the sample grep root widens from e2e/posix_spec/ to e2e/ so Chapter 4
  tests in e2e/builtin/ are not excluded.
- L115: Reported as not actionable. yosh's $0 = shell binary path
  diverges from bash/sh/dash (which use the script path), but POSIX
  §2.5.2 ("name of the shell or shell script") permits both readings.
  No code change required.

L113 and L114 remain in the section — they are independent large
projects (Chapter 4/8 systematic expansion; Chapter 2 normative-clause
depth) that need their own design specs.

Task context: requested via "/superpowers:brainstorming TODO.md の E2E
Test Expansion を全て対応して下さい。もし対応不要そうなのがあれば
報告して下さい。" — full assessment in
docs/superpowers/specs/2026-05-12-e2e-fd-close-readme-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3: Verify the commit landed**

Run:
```bash
git log --oneline -1 && git status
```

Expected:
- The newest commit's subject matches `test(e2e): strengthen fd_close + document Chapter 4 POSIX_REF form`.
- `git status` reports a clean working tree.

---

## Self-Review Result

- Spec coverage: §3.1 → Task 1; §3.2 → Task 2; §3.3 → Task 3; §4 → Task 4; §5 → Task 5. L115 ("not actionable" reporting) is realized in the commit message at Task 5 Step 2.
- Placeholder scan: no `TBD` / `TODO` / vague "handle errors" steps. All commands and file contents are exact.
- Type consistency: not applicable (no code, only text edits). File paths are consistent across tasks.
- One nit fixed inline during review: Task 3 Step 3/4 reference "current L115" and "current L116" to acknowledge that TODO.md line numbers will already have shifted by the time the editor opens the file — the bullet-text matching in those steps is what makes the operation robust to renumbering.
