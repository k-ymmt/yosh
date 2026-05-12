# E2E Test Expansion Cleanup — fd_close strengthening + README POSIX_REF contract

**Date:** 2026-05-12
**Status:** Approved (design phase)
**Source TODOs:** `TODO.md` "Future: E2E Test Expansion" section
(current L112, L115, L116 — line numbers will shift as the section drains)

## 1. Purpose

Drain three of the five items under `TODO.md` → `Future: E2E Test Expansion`:

- **L112** — `e2e/redirection/fd_close.sh` currently asserts only `EXPECT_EXIT: 0`.
  Strengthen the assertion so the test actually verifies the redirection
  semantics it claims to cover.
- **L116** — `e2e/README.md` POSIX_REF Format Contract (L89–108) lists four
  §2.X shapes only. Chapter 4 utilities now use the form
  `4 Utilities - <name>` (20+ files in `e2e/builtin/`). The grep example at
  L106 also points at `e2e/posix_spec/` which excludes `e2e/builtin/`.
- **L115** — Report as **not actionable**. yosh's `$0` behavior is
  POSIX-compliant per §2.5.2 ("name of the shell or shell script"); the
  divergence from bash/sh/dash is a documented observation, not a bug. No
  code change planned.

Out of scope:
- **L113** (Chapter 4 + Chapter 8 systematic expansion) — independent
  large project, requires its own spec.
- **L114** (Chapter 2 normative-requirement granularity, +100–200 tests) —
  independent large project, requires its own spec.

## 2. Background

### L112 — `fd_close.sh` weak assertion

Current file (`e2e/redirection/fd_close.sh`):

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: File descriptor close with N>&-
# EXPECT_EXIT: 0
echo "to stderr" >&2 2>&-
```

POSIX §2.7 processes redirections **left to right**. The intended semantics
of `>&2 2>&-`:

1. `>&2` (shorthand for `1>&2`): duplicate fd 2 onto fd 1. fd 1 now points
   to fd 2's original destination.
2. `2>&-`: close fd 2.

After both operations, fd 1 still holds the dup of the original fd 2
destination (independent of fd 2 itself). `echo "to stderr"` writes to
fd 1 → reaches the original stderr destination. fd 2 in the echo process
is closed, but echo never tries to use it.

The current assertion (`EXPECT_EXIT: 0` only) does not distinguish this
correct behavior from a hypothetical regression where `2>&-` is processed
before `>&2` (which would close stderr before the dup, making fd 1 invalid).

A sibling test (`e2e/posix_spec/2_07_redirection/dup_output_close.sh`)
already verifies a different scenario — that an `exec`-closed fd causes
subsequent writes to fail — so there is no duplication.

### L116 — Missing Chapter 4 form + stale grep example

`e2e/README.md` L89–108 documents the `POSIX_REF` format. Today it lists:

- `2.X.Y <Section Name>`
- `2.10.2 Rule N - <Name>`
- `2.10.2 Rule N - <Name> (<discriminator>)`
- `2.10 Shell Grammar - <Topic>`

Missing: the Chapter 4 form. `grep -lR "POSIX_REF: 4 Utilities" e2e/`
finds 32 files using `POSIX_REF: 4 Utilities - <name>` (e.g.,
`POSIX_REF: 4 Utilities - test`, `POSIX_REF: 4 Utilities - cd`). New
Chapter 4 test authors today have to grep precedent to learn the form.

The sample grep at L106:
```sh
grep -RE 'POSIX_REF: 2\.10' e2e/posix_spec/
```
searches `e2e/posix_spec/` only. Chapter 4 tests live in `e2e/builtin/`,
which is excluded. Replacing the root with `e2e/` future-proofs the
example against new corpus directories.

### L115 — `$0` divergence as observed, not bug

yosh runs scripts with `$0 = argv[0]` of the shell binary. bash/sh/dash
run scripts with `$0 = script path`. POSIX §2.5.2 reads:

> 0 — Expands to the name of the shell or shell script. […]

Both readings are conformant. yosh ships one valid interpretation. The
TODO entry is a forward-looking note for any future change of mind; no
action today.

## 3. Changes

### 3.1 `e2e/redirection/fd_close.sh`

Replace contents with:

```sh
#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: Per-command "2>&-" after ">&2" closes fd 2; dup'd target on fd 1 survives
# EXPECT_STDERR: to stderr
# EXPECT_EXIT: 0
echo "to stderr" >&2 2>&-
```

Two changes:
- `DESCRIPTION` line clarifies the semantic under test.
- New `EXPECT_STDERR: to stderr` asserts (via substring) that "to stderr"
  actually reaches the stderr stream — i.e., the dup survived the
  subsequent close.

`EXPECT_OUTPUT:` is intentionally **omitted**. The harness parser
(`e2e/run_tests.sh` L223) matches `"# EXPECT_OUTPUT: "*` requiring a
trailing space; the no-trailing-space `# EXPECT_OUTPUT:` form silently
disables the stdout check rather than asserting empty stdout. Until that
harness quirk is fixed (out of scope here), we avoid adding a line that
appears to assert empty stdout but does not.

### 3.2 `e2e/README.md` — POSIX_REF Format Contract

Add a fifth bullet to the format list (after L101):

```markdown
- `4 Utilities - <name>` — for Chapter 4 utility tests (XCU Chapter 4).
  Example: `POSIX_REF: 4 Utilities - test`
```

Update the sample grep (L107) from:
```sh
grep -RE 'POSIX_REF: 2\.10' e2e/posix_spec/
```
to:
```sh
grep -RE 'POSIX_REF: 2\.10' e2e/
```

The narrower `e2e/posix_spec/` form excluded `e2e/builtin/`; using `e2e/`
captures all corpora and remains correct if future Chapter 8 / Chapter 4
expansion lands new directories.

### 3.3 `TODO.md`

Delete the three items from `Future: E2E Test Expansion`:
- L112 — completed
- L115 — reported as not actionable (mention in commit message)
- L116 — completed

Per project convention (`CLAUDE.md` → "Delete completed items rather than
marking them with `[x]`"), the lines are removed, not annotated.

L113 and L114 remain.

## 4. Verification

- `./e2e/run_tests.sh --filter=fd_close` — must show `[PASS]` for the
  edited file with the new substring check active.
- `./e2e/run_tests.sh` (full sweep) — confirm no regressions across the
  rest of the e2e corpus.
- `cargo test` — sanity check; the changes do not touch Rust code, but
  the project policy requires a passing test suite before commit.

## 5. Commit

Single commit on `main` (per memory: direct main work, no branch). Commit
message includes:
- Title summarizing both fixes
- Body referencing TODO L112 / L115 / L116 with brief rationale
- Note that L115 was assessed as not-actionable (POSIX §2.5.2 permits
  yosh's behavior)
- Original task context for traceability (per user global CLAUDE.md)

## 6. Risk

Minimal:
- The `EXPECT_STDERR` substring is "to stderr" — unique enough that no
  unrelated stderr noise (e.g., plugin-load warnings observed in local
  runs) would falsely match.
- README changes are documentation only.
- TODO.md deletions match the project's established cleanup pattern.
