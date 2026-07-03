# TODO.md Performance Items Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve all 22 PERF items in TODO.md's "Performance" section (Audit 2026-07-02) without changing observable shell behavior.

**Architecture:** The items cluster into four file-disjoint groups executed sequentially: (1) executor/env exec-cache cluster, (2) expansion hot paths, (3) interactive line-editor redraw, (4) lexer/parser allocation churn. Each task is behavior-preserving; the existing unit + integration + E2E suites are the regression gate, with new focused unit tests added where a fix could plausibly change semantics (LINENO, environ cache, field splitting, strip forms).

**Tech Stack:** Rust (stable), cargo test, ./e2e/run_tests.sh, criterion benches (`cargo bench`) for before/after spot checks where cheap.

## Global Constraints

- POSIX compliance is the primary goal; every optimization must preserve observable behavior (verified by full `cargo test` + `./e2e/run_tests.sh`).
- Do NOT use `cargo build --workspace` / `cargo test --workspace` (wasm crates fail to host-build).
- Error messages prefixed `yosh: `; exit-code conventions unchanged.
- Delete completed items from TODO.md (no `[x]` markers).
- Commit at the end of each task with the original task context in the message.
- Full test suite is slow (~minutes): run `cargo test` in background per memory guidance.

---

### Task 1: Executor exec/environ-cache cluster (TODO items 1–7)

**Files:**
- Modify: `src/env/vars.rs` (set / set_with_options / build_environ / environ_cache invalidation)
- Modify: `src/env/mod.rs` (LINENO integer field if stored on ShellEnv)
- Modify: `src/exec/simple.rs` (LINENO write at :113, build_env_vars at :515, execvp at :593, cmd_str_for_hooks at :266)
- Modify: `src/expand/param.rs` (intercept `$LINENO` lookup)
- Test: unit tests in `src/env/vars.rs`, `tests/` integration; E2E suite

**Items (from TODO.md):**
1. `set("LINENO", …)` per simple command thrashes environ_cache → store LINENO as integer field on ShellEnv, intercept `$LINENO` in `expand::param` (and `set`/`env`-style listings if they surface LINENO today — verify current behavior first and preserve it).
2. `set`/`set_with_options` invalidate `environ_cache` unconditionally → only clear when the written variable is/was exported (new vars in single-scope fast path: not exported unless allexport).
3. `set` re-inserts a fresh `Variable` + freshly allocated key to update existing vars → use `get_mut` and mutate `value` in place (both fast path and multi-scope loop, and `set_with_options`).
4. `build_environ` builds a temp `HashMap<String, &Variable>` cloning all names → iterate scopes top-down with a `HashSet<&str>` of seen keys, clone only exported entries.
5. `build_env_vars` clones entire environ via `environ().to_vec()` per external command → skip the clone when no prefix assignments; merge via map otherwise.
6. External exec calls `execvp` (child re-walks PATH) → resolve via `find_in_path` in the parent, `execv` the absolute path. Preserve ENOEXEC / not-found / not-executable exit codes (127/126) and PATH-less (contains `/`) behavior.
7. `cmd_str_for_hooks` builds a joined string even with no plugin hooks → gate on hooks being registered.

- [ ] **Step 1:** Read all touched code; write failing/locking unit tests for: LINENO expansion inside scripts (`echo $LINENO` on line N), environ cache invalidation on exported vs non-exported writes, in-place `set` preserving export/readonly semantics, `build_environ` scope shadowing.
- [ ] **Step 2:** Implement items 2–4 in `src/env/vars.rs` (cache gating, get_mut in-place update, single-pass build_environ).
- [ ] **Step 3:** Implement item 1 (LINENO integer field + param intercept), keeping `LINENO` assignable by user scripts if it is today (verify with dash/bash semantics: user assignment overrides until next command? Match current yosh observable behavior per existing tests/e2e).
- [ ] **Step 4:** Implement items 5–7 in `src/exec/simple.rs`.
- [ ] **Step 5:** `cargo test` (background) + `./e2e/run_tests.sh`; fix regressions.
- [ ] **Step 6:** Delete resolved items from TODO.md; commit.

### Task 2: Expansion hot paths (TODO items 8–10)

**Files:**
- Modify: `src/expand/field_split.rs:9,36` (IFS allocation fast path)
- Modify: `src/expand/param.rs:194,211` (strip_prefix/strip_suffix literal fast path)
- Modify: `src/expand/pattern.rs:104` (pre-parse bracket / pattern tokens)
- Test: unit tests in each module

**Items:**
8. `split()` allocates IFS String + two Vec<u8> per word even when nothing splits → read IFS by reference; short-circuit when IFS is default and no IFS bytes present.
9. `${x##*/}` / `${x%.*}` strip forms run anchored matches at every boundary (O(n²)+) → fast path for literal (metachar-free) patterns; anchored prefix/suffix scanning.
10. `parse_bracket` re-parses class body per call; `*[abc]x` re-parses O(n) times → pre-parse pattern into tokens once per match call.

- [ ] **Step 1:** Add unit tests locking current strip/split/glob behavior on edge cases (empty IFS, IFS unset vs empty, multibyte, longest/shortest match, bracket classes after `*`).
- [ ] **Step 2:** Implement each item; keep public APIs stable.
- [ ] **Step 3:** `cargo test` (background) + targeted `./e2e/run_tests.sh --filter=` for expansion; fix regressions.
- [ ] **Step 4:** Delete resolved items from TODO.md; commit.

### Task 3: Interactive redraw cluster (TODO items 11–15)

**Files:**
- Modify: `src/interactive/line_editor.rs:576,602,639,412` (redraw, update_suggestion)
- Modify: `src/interactive/highlight_scanner/mod.rs:96,113` (scan clone churn)
- Test: unit tests in `src/interactive/`, `tests/pty_interactive.rs` (PTY tests can be flaky; generous timeouts)

**Items:**
11. `redraw` per-char `spans.iter().find(...)` → advance a single span-cursor alongside char index (spans sorted, non-overlapping).
12. `redraw` full clear+repaint per keystroke → repaint from scanner's `diff_pos` first changed column.
13. `redraw` re-sums Unicode widths over whole buffer + cursor prefix per keystroke → maintain incremental prefix/total width totals on edit.
14. highlight `scan` clones full spans + input into cache per keystroke; append fast-path clones prior spans → reuse owned buffers via `std::mem::take`, push only newly scanned tail.
15. `update_suggestion` rebuilds full-line String per keystroke including cursor-movement keys → skip for actions that can't change the suggestion.

- [ ] **Step 1:** Read the redraw/scanner code fully; identify existing unit-test seams (span classification, diff_pos) and add tests for incremental width bookkeeping and span-cursor classification equivalence.
- [ ] **Step 2:** Implement items 11–15 incrementally, running `cargo test --test interactive` and unit tests between steps.
- [ ] **Step 3:** Run PTY tests (`cargo test --test pty_interactive`) and manually sanity-check rendering paths covered by tests.
- [ ] **Step 4:** Delete resolved items from TODO.md; commit.

### Task 4: Lexer/parser allocation churn (TODO items 16–22)

**Files:**
- Modify: `src/lexer/alias.rs:8` (VecDeque)
- Modify: `src/lexer/scanner.rs:306` + `src/parser/mod.rs:270` (light save_state)
- Modify: `src/parser/simple.rs:22,95`, `src/parser/word.rs:8`, `src/parser/mod.rs:270,310` (token clone removal, &str binding, as_literal once)
- Modify: `src/lexer/word.rs:136` (skip parts rebuild)
- Modify: `src/lexer/heredoc.rs:62` (borrowed delimiter compare)
- Test: existing lexer/parser unit tests; full suite

**Items:**
16. alias token dequeue `first().cloned()` + `remove(0)` → `VecDeque::pop_front`.
17. `try_read_io_number` / assignment look-ahead `save_state()` clones queue+HashSet → snapshot only `pos`/`line`/`column` for the common no-op restore (only valid when no alias-queue mutation can occur in the peeked region — verify; otherwise keep full save on the rare path).
18. parser double-clones current Word token → single move via `std::mem::replace`.
19. `try_parse_assignment` clones first literal String but only slices it → bind `&str`.
20. `is_complete_command_end` / `is_compound_command_start` recompute `as_literal()` up to 8× → compute once, `match`.
21. `read_word_parts` rebuilds parts Vec to drop empty literals → skip rebuild when none produced.
22. `read_heredoc_body` allocates per-line String for delimiter compare → compare `&str` borrow.

- [ ] **Step 1:** Implement items 16, 19, 20, 21, 22 (mechanical, low-risk) — run lexer/parser unit tests.
- [ ] **Step 2:** Implement items 17 and 18 carefully (state-restore correctness, borrow-checker restructuring); add a unit test covering alias expansion across `save_state`/`restore_state` if the light snapshot changes any path.
- [ ] **Step 3:** Full `cargo test` (background) + `./e2e/run_tests.sh`; fix regressions.
- [ ] **Step 4:** Delete resolved items from TODO.md; commit.

### Task 5: Final verification & wrap-up

- [ ] **Step 1:** `cargo bench` spot-check (no formal target; confirm no regression on parser/expansion benches, note improvements).
- [ ] **Step 2:** Full `cargo test` + `./e2e/run_tests.sh` green.
- [ ] **Step 3:** Confirm TODO.md Performance section is empty (delete the now-empty section header per TODO.md convention).
- [ ] **Step 4:** `cargo fmt` + final commit.
