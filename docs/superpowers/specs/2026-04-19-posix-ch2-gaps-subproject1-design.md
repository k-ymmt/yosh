# POSIX Chapter 2 Conformance Gaps — Sub-project 1: `<&`, `<>`, `times` E2E Coverage

**Date**: 2026-04-19
**Sub-project**: 1 of 5 (POSIX Chapter 2 conformance gap remediation)
**Scope items from TODO.md**:

- §2.7.5 Duplicating an Input File Descriptor (`<&`) — no dedicated test
- §2.7.7 Open File Descriptors for Reading and Writing (`<>`) — no dedicated test
- §2.14.13 `times` builtin — TODO.md says "not implemented" but it is (stale entry)

## Context

TODO.md lists a "Future: POSIX Conformance Gaps (Chapter 2)" section
enumerating eight items. Investigation showed these items are of mixed
kinds: some require implementation work, some only test coverage, and
one requires harness scaffolding. This sub-project addresses the three
lightest, fully independent items — all test-only additions — to
establish the `e2e/posix_spec/2_07_*` and `e2e/posix_spec/2_14_*`
directories that later sub-projects will extend.

Investigation findings:

- `RedirectKind::DupInput` is implemented at `src/exec/redirect.rs:135`
  and `RedirectKind::ReadWrite` at `src/exec/redirect.rs:155`. The
  parser emits both kinds. Only dedicated E2E tests are missing.
- `builtin_times()` is implemented at `src/builtin/special.rs:466`,
  classified as Special at `src/builtin/mod.rs:33`. The TODO.md note
  "§2.14.13 times builtin not implemented" is stale.
- `e2e/redirection/` already exists with file-redirection tests
  (`input_redirect.sh`, `output_redirect.sh`, etc.) but nothing for
  `<&` or `<>`.
- The `e2e/posix_spec/` tree is the canonical home for POSIX chapter-
  ordered tests (§2.6.1, §2.10, §2.11 already live there). New Chapter
  2 gap tests belong here, not in the feature-named legacy dirs.
- `e2e/run_tests.sh` supports `EXPECT_OUTPUT`, `EXPECT_EXIT`,
  `EXPECT_STDERR`, and `XFAIL` metadata. `EXPECT_STDERR` match
  semantics will be verified at implementation start; in-script `case`
  verification is the fallback for non-deterministic or error output.

## Goals

1. Establish `e2e/posix_spec/2_07_redirection/` with dedicated §2.7.5
   and §2.7.7 coverage (8 files: 4 `dup_input_*.sh`, 4 `readwrite_*.sh`).
2. Establish `e2e/posix_spec/2_14_13_times/` with §2.14.13 coverage
   (3 files).
3. Remove the three addressed items from TODO.md's "Future: POSIX
   Conformance Gaps (Chapter 2)" section when all tests pass; XFAIL
   and re-file any item whose test uncovers real implementation gaps.
4. Correct the stale "§2.14.13 times builtin not implemented" note.

## Non-goals

- Implementation changes to `src/`. If an edge-case test fails because
  the implementation deviates from POSIX, mark it `XFAIL` with a
  specific reason and defer the fix to a dedicated sub-project.
- §2.7.6 `>&` (DupOutput) coverage. Defer to a follow-up sub-project.
- §2.7.1–§2.7.4 coverage. Existing `e2e/redirection/*` covers these
  functionally; a future "Chapter 2 hybrid coverage" sub-project will
  migrate them with `POSIX_REF` headers.
- Other Chapter 2 gaps (§2.6.1 mixed-WordPart tilde, §2.6.1 escape
  preservation, §2.10.1/2 grammar, §2.11 ignored-on-entry). These are
  sub-projects 2–5.

## Architecture

Single-directory test additions. No code changes in `src/`. No changes
to `e2e/run_tests.sh`. Two new directories:

```
e2e/posix_spec/
├── 2_07_redirection/          (new)
│   ├── dup_input_basic.sh
│   ├── dup_input_param_expansion.sh
│   ├── dup_input_bad_fd.sh
│   ├── dup_input_close.sh
│   ├── readwrite_basic.sh
│   ├── readwrite_creates_file.sh
│   ├── readwrite_param_expansion.sh
│   └── readwrite_bidirectional.sh
└── 2_14_13_times/             (new)
    ├── times_exit_code.sh
    ├── times_format.sh
    └── times_in_subshell.sh
```

All files: mode `644`, `#!/bin/sh` first line, `POSIX_REF` + `DESCRIPTION`
headers.

## Test Inventory

### §2.7.5 `<&` — `e2e/posix_spec/2_07_redirection/`

| File | Scenario | Verification |
|---|---|---|
| `dup_input_basic.sh` | `exec 3<tmpfile; cat <&3; exec 3<&-` — duplicate fd 3 into cat's stdin | `EXPECT_OUTPUT: <tmpfile contents>` |
| `dup_input_param_expansion.sh` | `exec 3<tmpfile; fd=3; cat <&"$fd"; exec 3<&-` — fd number via parameter expansion | `EXPECT_OUTPUT` match |
| `dup_input_bad_fd.sh` | `cat <&9` where fd 9 is not open | non-zero exit; stderr contains `yosh:` (in-script `case` check) |
| `dup_input_close.sh` | `exec 3<tmpfile; exec 3<&-; cat <&3` — re-using a closed fd fails | non-zero exit on the post-close read (in-script `case` check) |

### §2.7.7 `<>` — `e2e/posix_spec/2_07_redirection/`

| File | Scenario | Verification |
|---|---|---|
| `readwrite_basic.sh` | `f=/tmp/yosh_$$_rw; echo hi 1<>"$f"; cat "$f"; rm -f "$f"` | `EXPECT_OUTPUT: hi` |
| `readwrite_creates_file.sh` | `<>` opens a non-existent file by creating it | file exists after; exit 0 |
| `readwrite_param_expansion.sh` | fd number and filename both via parameter expansion | data round-trips |
| `readwrite_bidirectional.sh` | Open with `<>`, write, then read. POSIX does not rewind, so assert only that the operation succeeds — do not assert read content | exit 0 |

Cleanup: every file using a tempfile removes it at the end with
`rm -f`. No `trap` (linear cleanup suffices for E2E).

### §2.14.13 `times` — `e2e/posix_spec/2_14_13_times/`

| File | Scenario | Verification |
|---|---|---|
| `times_exit_code.sh` | `times; echo $?` | `EXPECT_OUTPUT: 0` |
| `times_format.sh` | Capture `times` output, verify two lines of `NmS.sssssS NmS.sssssS` shape via `case` glob | `EXPECT_EXIT: 0` |
| `times_in_subshell.sh` | `( times )` inside a subshell still prints two-line shape | `EXPECT_EXIT: 0` |

`times_format.sh` and `times_in_subshell.sh` omit `EXPECT_OUTPUT`
because CPU-time values are non-deterministic. A 1-line comment in
each file explains the omission (mirroring the convention used for
`tilde_rhs_user_form.sh`).

## Verification Strategy

**Deterministic output** → use `EXPECT_OUTPUT`.
**Non-deterministic output** → omit `EXPECT_OUTPUT`, verify shape in-
script with `case` against a glob pattern, set `EXPECT_EXIT: 0`, and
document the omission with a comment.
**Error output / non-zero exit** → capture stderr with `2>&1` into a
variable and verify with `case "$err" in *yosh:*) ...`. This avoids
coupling to `EXPECT_STDERR`'s exact matching semantics until those are
confirmed by reading `e2e/run_tests.sh`.

All pattern checks use POSIX `case` glob (`*`, `?`, `[...]`) — no
regex, no `grep -E`. This keeps tests portable and matches the style
of existing `tilde_rhs_user_form.sh`.

## Workflow

### Step 0 — Harness pre-check

Read `e2e/run_tests.sh` to confirm `EXPECT_STDERR` match semantics
(exact vs substring). Decide between `EXPECT_STDERR` and in-script
capture before writing error-path tests. Confirm no `dup_output_*`
files exist in `e2e/posix_spec/2_07_*` (they don't yet; this is a
greenfield directory).

### Step 1 — `times` tests (lowest risk)

1. Create `e2e/posix_spec/2_14_13_times/` (mode 644 files).
2. Add `times_exit_code.sh`, `times_format.sh`, `times_in_subshell.sh`.
3. Run `./e2e/run_tests.sh --filter=2_14_13_times` — expect 3/3 pass.
4. Remove TODO.md line "§2.14.13 times builtin not implemented".

### Step 2 — `<&` tests

1. Create `e2e/posix_spec/2_07_redirection/`.
2. Add files in order: `dup_input_basic.sh`,
   `dup_input_param_expansion.sh`, `dup_input_bad_fd.sh`,
   `dup_input_close.sh`.
3. After each, `./e2e/run_tests.sh --filter=dup_input`.
4. Any failure → add `# XFAIL: <reason>` header and note in TODO.md
   under "Future: POSIX Conformance Gaps (Chapter 2)".

### Step 3 — `<>` tests

1. Same directory. Add `readwrite_basic.sh`,
   `readwrite_creates_file.sh`, `readwrite_param_expansion.sh`,
   `readwrite_bidirectional.sh`.
2. Same filter-and-verify pattern, same XFAIL escape.

### Step 4 — Consolidation

1. `./e2e/run_tests.sh` (full run) — verify no regressions.
2. `cargo test` — verify no Rust regressions.
3. Update TODO.md:
   - Remove cleanly-passing items.
   - Rewrite any XFAIL'd item with a specific, technical reason.
4. Commit per step (4 commits total) following existing history style
   (`test(redir): add §2.7.5 dup-input E2E coverage`, etc.).

## Success Criteria

1. `./e2e/run_tests.sh --filter=2_07_redirection` — 8 files, `FAIL=0`
   (pass + XFAIL = 8).
2. `./e2e/run_tests.sh --filter=2_14_13_times` — 3 files, `FAIL=0`.
3. Full `./e2e/run_tests.sh` — no regressions in previously passing
   tests.
4. `cargo build && cargo test` — clean.
5. TODO.md "Future: POSIX Conformance Gaps (Chapter 2)" section:
   - §2.7.5, §2.7.7, §2.14.13 lines removed on clean pass.
   - Any XFAIL'd item rewritten with a specific implementation-level
     reason (not a generic "test missing" note).
6. All new files: mode `644`, correct shebang, `POSIX_REF` + `DESCRIPTION`
   headers.

## File Conventions

Per CLAUDE.md and existing E2E style:

```sh
#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <one-line description>
# EXPECT_EXIT: 0
# (optional) # EXPECT_OUTPUT: ...
# (optional) # XFAIL: <reason>
<test body>
```

Omitted `EXPECT_OUTPUT` (for `times_format.sh`, `times_in_subshell.sh`,
and error-path tests): add a one-line comment explaining the omission,
as established in `tilde_rhs_user_form.sh`.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| `EXPECT_STDERR` match semantics unclear | Pre-check in Step 0; default to in-script `case` capture |
| `<&-` close semantics diverge from POSIX | XFAIL with specific reason; defer fix to a dedicated sub-project |
| `<>` `O_CREAT` behavior diverges from POSIX | XFAIL with specific reason; defer fix |
| `times_format.sh` glob pattern too loose and would pass garbage | Use `*m*s\ *m*s` anchored by the literal `m` and `s` and intervening space; no test is a strict formal verifier, the goal is shape regression detection |
| `/tmp/yosh_$$_rw` collision between parallel runs | Include `$$` pid suffix; cleanup with `rm -f` is idempotent |

## Out of Scope (explicit)

- Implementation fixes in `src/exec/redirect.rs` or
  `src/builtin/special.rs`. If any uncovered, file a TODO item and
  XFAIL the test.
- Migrating `e2e/redirection/*.sh` to `POSIX_REF` headers. Belongs to
  sub-project 2 (Chapter 2 hybrid coverage).
- `>&` (DupOutput) tests. Follow-up sub-project.
- Documentation updates beyond TODO.md.
