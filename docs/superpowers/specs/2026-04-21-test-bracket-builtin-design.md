# `test` / `[` Builtin Promotion — Design

**Date:** 2026-04-21
**Related:** `performance.md` §4.1 (P0), `TODO.md` (no existing entry)
**Prompt:** "performance.md の中から優先度が高そうなものを対応してください" → scope narrowed to §4.1 after root-cause investigation revealed that the report's cited cause was incorrect.

## 1. Motivation

### 1.1 Original finding (as stated in `performance.md` §4.1)

The W2 dhat profile attributes ~52 MB / ~380k allocations to `VarStore::build_environ` + `Executor::build_env_vars`. The report's recommended fix was "skip `build_env_vars` for builtins."

### 1.2 Root-cause correction

Code inspection (`src/exec/simple.rs:383`) shows `build_env_vars` is already called **only on the `NotBuiltin` dispatch path**, i.e. for external commands. The report's premise — that builtins also pay this cost — is factually wrong.

The **real driver** is that `classify_builtin` (`src/builtin/mod.rs:30-38`) does **not** list `test` / `[`. W2's `benches/data/script_heavy.sh` Section B contains:

```sh
i=0
while [ "$i" -lt 1000 ]; do
    greet "world" > /dev/null
    i=$((i + 1))
done
```

Each loop iteration invokes `[` — which, classified as `NotBuiltin`, goes through `fork`+`execvp`+`build_env_vars`. This produces ~1001 external-command invocations per W2 run, matching the dhat rank 5 site (1001 outer-`Vec` allocations at `src/exec/simple.rs:406:48`).

### 1.3 Why this is a P0 fix

- POSIX explicitly permits `test` / `[` as builtins (§2.14), and every major shell (bash, dash, ash, ksh, zsh, mksh) implements them so. Making them external is a correctness-neutral but performance-catastrophic choice.
- Eliminates 1001+ `fork`+`execvp` per W2 run — expected ≥100× speedup on `exec_bracket_loop_200` (to be added) and substantial reduction of W2's 68 MB total allocation.
- Low risk: `test` semantics are POSIX-specified with a small, well-defined surface area.

## 2. Architecture & File Layout

**New file:** `src/builtin/test.rs`
- `pub fn builtin_test(name: &str, args: &[String]) -> i32`
  - `name` is `"test"` or `"["`. For `"["`, the last arg must be `"]"`; it is stripped before evaluation.
- Private `evaluate(args: &[&str]) -> Result<bool, TestError>` dispatches by operand count (0..=4).
- Private `TestError { message: String, exit_code: i32 }` encapsulates operator/arity errors.
- stderr is emitted by `builtin_test` (not inside `evaluate`) as `yosh: {name}: {message}` for consistency with other builtins.

**Modified files:**
- `src/builtin/mod.rs`:
  - Add `"test" | "["` to the `Regular` arm of `classify_builtin`.
  - Add dispatch for `"test"` / `"["` in `exec_regular_builtin`, calling `test::builtin_test`.
- `src/builtin/regular.rs`: **unchanged** (kept separate to avoid hitting ~900 → 1200+ LOC; `test` is substantial enough to warrant its own module).

**Dependencies:**
- `std::fs::{metadata, symlink_metadata}` — file existence + stat
- `std::os::unix::fs::MetadataExt` — setuid/setgid (`-u`, `-g`)
- `nix::unistd::{isatty, access, AccessFlags}` — `-t`, plus `-r`/`-w`/`-x` (POSIX requires effective-UID checks, so `access(2)` is mandatory)

**Why new module not extended `regular.rs`:** `regular.rs` is already ~900 LOC. Adding ~300 LOC of `test` logic would push it toward the file-size threshold flagged in our design principles ("when a file grows large, that's often a signal that it's doing too much"). A focused `test.rs` is easier to read, test, and evolve.

## 3. Evaluation Semantics (POSIX §2.14)

Dispatch by operand count:

| Count | Form | Evaluation |
|---|---|---|
| 0 | `test` | false (exit 1) |
| 1 | `test S` | true iff `S` is nonempty |
| 2 | `! S` | negation of 1-operand form on `S` |
| 2 | `-n S` / `-z S` / `-<fileop> F` | unary operator |
| 3 | `S1 op S2` | binary string (`=`, `!=`) or integer (`-eq`/`-ne`/`-lt`/`-gt`/`-le`/`-ge`) |
| 3 | `! E` | negation of 2-operand form on `E` |
| 3 | `( E )` | 1-operand form evaluated on inner `E` |
| 4 | `! E` | negation of 3-operand form |
| 4 | `( E )` | 2-operand form on inner |
| ≥5 | — | exit 2 + `yosh: {name}: too many arguments` (POSIX unspecified; we reject explicitly) |

### 3.1 Operator set

- **Unary file tests:** `-b`, `-c`, `-d`, `-e`, `-f`, `-g`, `-h`, `-L`, `-p`, `-r`, `-S`, `-s`, `-t`, `-u`, `-w`, `-x`
  - `-h` and `-L` are equivalent (both test symbolic link, per POSIX)
  - `-t FD` calls `isatty(FD)` on the given file descriptor number
  - `-r` / `-w` / `-x` use `access(2)` via `nix::unistd::access`. Note: POSIX `access(2)` uses the **real** UID/GID, not effective. This matches bash/dash behavior and is acceptable for POSIX §2.14 purposes (POSIX defers to `access(2)` semantics).
- **Unary string:** `-n` (nonempty), `-z` (empty)
- **Binary string:** `=`, `!=`
- **Binary integer:** `-eq`, `-ne`, `-lt`, `-gt`, `-le`, `-ge`

### 3.2 Out of scope (not implemented)

- `-a`, `-o`, complex `(` `)` nesting beyond what 3/4-operand forms cover — POSIX marks these obsolescent in §2.14. Users should compose with `&&` / `||`.
- `<`, `>` (bash string lexicographic) — not POSIX.

### 3.3 Integer parsing rules

- Leading and trailing whitespace stripped before parse (POSIX: "the operand shall be an integer")
- Signed (`+`, `-`) allowed
- On parse failure: exit 2 + `yosh: {name}: <val>: integer expression expected`

### 3.4 `!` negation rule

A leading `!` as the first operand is treated as negation. A trailing `!` or `!` not in first position is a literal string (handled by the normal operand count dispatch). This matches bash/dash behavior and POSIX §2.14.

### 3.5 `[` closing bracket

If `name == "["`, the last element of `args` must equal `"]"`. Otherwise: exit 2 + `yosh: [: missing ']'`. After validation, `]` is dropped and evaluation proceeds with the remaining operands.

## 4. Error Handling & Exit Codes

| Situation | exit | stderr |
|---|---|---|
| Expression true | 0 | — |
| Expression false | 1 | — |
| File-test operand that refers to a nonexistent / inaccessible path | 1 (false) | — (silent, per POSIX) |
| Unknown operator (e.g. `-Z`) | 2 | `yosh: {name}: -Z: unknown operator` |
| Integer parse failure | 2 | `yosh: {name}: <val>: integer expression expected` |
| `[` without closing `]` | 2 | `yosh: [: missing ']'` |
| Operand count > 4 | 2 | `yosh: {name}: too many arguments` |
| Binary operator missing RHS | 2 | `yosh: {name}: argument expected` |

**Return type:** `builtin_test` returns `i32`, not `Result<i32, ShellError>`. Rationale: `test` exit statuses (including 2) are not flow-control errors; they are regular command exit statuses. Matches existing `builtin_echo` signature.

**`set -e` interaction:** `test` exit status 2 can trigger errexit per POSIX. The existing errexit-suppression machinery (inside `if`/`while`/`until` conditions, `&&`/`||` LHS, etc.) automatically handles the common cases — no additional work needed.

## 5. Testing Strategy

### 5.1 Unit tests (`src/builtin/test.rs` `mod tests`)

Organized by operand count. Target: ~60 cases.

- **0/1 operand:** `test`, `test ""`, `test "x"`, `test " "`
- **2 operand:**
  - `! ""`, `! "x"` (negation of 1-arg form)
  - Each unary string/file op — use `tempfile::NamedTempFile` / `tempfile::tempdir` for filesystem cases
  - Permission-based cases (`-r`, `-w`, `-x`) with `std::fs::set_permissions`
- **3 operand:**
  - Integer boundary: `"0" -eq "0"`, `" 42 " -eq "42"`, negatives, parse failures
  - String `=` / `!=`
  - `! -n "x"`, `! -z ""`
  - `( "x" )` → true, `( "" )` → false
- **4 operand:**
  - `! -n "x"` (negate 2-arg)
  - `! ( "x" )`
  - `( -n "x" )`, `( "a" = "b" )`
- **Edge cases:**
  - `[` with/without `]`
  - `[ "]" ]` (single `]` operand inside brackets)
  - `test [ ]` (when `[` and `]` appear as operands to literal `test`)
  - `test !` (just `!` is 1-operand form → true, since `"!"` is nonempty)

### 5.2 Criterion benchmark (`benches/exec_bench.rs`)

New bench `exec_bracket_loop_200`:

```rust
const BRACKET_LOOP_SCRIPT: &str = r#"
i=0
while [ "$i" -lt 200 ]; do
    i=$((i + 1))
done
"#;

// inside bench_exec:
c.bench_function("exec_bracket_loop_200", |b| {
    b.iter(|| {
        let status = run_script(black_box(BRACKET_LOOP_SCRIPT));
        assert_eq!(status, 0);
    });
});
```

Reuses the existing `run_script` helper in `benches/exec_bench.rs:7`.

Purpose: regression gate for the optimization. Pre-implementation baseline captured on `main`'s current tip via `cargo bench --save-baseline pre-bracket-builtin`; after the feature branch lands, compare with `cargo bench --baseline pre-bracket-builtin`.

**Additional expected effect:** `exec_function_call_200` (`benches/exec_bench.rs:23-30`) uses `while [ "$i" -lt 200 ]` internally. This bench will also speed up substantially — meaning the reported 187× ratio in `performance.md` §4.2 was partly attributable to `[` being external, not purely to function-call overhead. Re-measuring this bench post-fix is required to re-quantify the genuine §4.2 overhead.

### 5.3 E2E tests (`e2e/posix_spec/2_14_test/`)

New directory with ~15 representative tests. All files with `POSIX_REF: 2.14 test`, `644` permissions, metadata headers per `CLAUDE.md`:

- `test_no_args.sh` — 0 operand → exit 1
- `test_string_nonempty.sh` — 1-operand true/false
- `test_bracket_requires_closing.sh` — `[ -n x` → exit 2
- `test_integer_compare.sh` — `-eq` / `-lt` / `-ge`
- `test_integer_parse_error.sh` — `[ abc -eq 0 ]` → exit 2
- `test_file_exists.sh` — `-e`
- `test_file_regular.sh` — `-f`
- `test_file_readable.sh` — `-r` with permission manipulation
- `test_file_symlink.sh` — `-h` / `-L` equivalence
- `test_string_eq_neq.sh`
- `test_negation.sh` — `! -z ""`, `! ( -n "x" )`
- `test_paren_grouping.sh` — `( -n "x" )`, `( "a" = "b" )`
- `test_unknown_operator.sh` — `-Z foo` → exit 2
- `test_isatty_fd.sh` — `-t 0 </dev/null` → false
- `test_too_many_args.sh` — operand count > 4 → exit 2

### 5.4 Regression coverage

- `cargo test --workspace` — full workspace pass
- `./e2e/run_tests.sh` — all existing E2E pass, including the 10+ files already using `[`
- `cargo bench exec_bracket_loop_200` — shows ≥10× improvement over pre-implementation baseline (target: ≥100× given no fork/exec)

## 6. Performance Verification & Report Updates

### 6.1 Post-implementation measurement flow

1. **Before** starting implementation: on `main`'s current tip, run `cargo bench --save-baseline pre-bracket-builtin` to capture the Criterion baseline.
2. Land implementation + tests on the feature branch.
3. Run `cargo bench --baseline pre-bracket-builtin` to produce the comparison report.
4. Re-run W2 dhat profile:
   ```
   cargo run --profile profiling --features dhat-heap --bin yosh-dhat -- \
       benches/data/script_heavy.sh
   mv dhat-heap.json target/perf/dhat-heap-w2-post-bracket.json
   python3 scripts/perf/dhat_top_n.py target/perf/dhat-heap-w2-post-bracket.json 10
   ```
5. Confirm `exec_for_loop_200`, `exec_function_call_200`, `exec_param_expansion_200` in the step-3 comparison — they should show no regression, and `exec_function_call_200` will likely show substantial improvement (see §5.2 note).

### 6.2 `performance.md` edits

- **§1 Executive Summary**:
  - Rewrite hotspot #1 description: "`[` / `test` was dispatched as an external command per while-loop iteration"
  - Update byte/call totals with post-fix measurements.
- **§4.1**:
  - Replace the "Suspected cause" paragraph. Correct the claim that `build_env_vars` runs for builtins.
  - State the actual cause: `classify_builtin` omitted `test` / `[`, causing 1001 `fork`+`execvp` per W2.
  - Mark fix candidate #1 as "already implemented (builtin skip was in place); no benefit."
  - Add fix candidate #0 (executed): "Promote `test` / `[` to `Regular` builtin" with cross-reference to this spec / commit.
- **§3.2 dhat Top-10**: replace with post-fix measurements. `pathname::expand` / `pattern::matches` / `field_split::emit` should rise as the new top hotspots.
- **§5.1 Priority matrix**: mark §4.1 as completed with commit reference (date stamped at implementation time).
- **§5.2 Next-project queue**: promote §4.2 (function-call 187×) to the top of the remaining P0 queue.
- **New §4.6 (appended)**: "Correction to §4.1 analysis" — capture the original mischaracterization and the real driver, so future readers have an audit trail.

### 6.3 `TODO.md` edits

- The existing `LINENO` entry is unrelated; leave as-is.
- No new entry for `[` / `test` builtin (completed, not deferred).

### 6.4 Commit granularity

1. `feat(builtin): implement test / [ as POSIX-compliant builtins` — implementation + unit tests + classify_builtin dispatch
2. `test(e2e): add POSIX §2.14 test coverage for [/test builtins` — e2e/posix_spec/2_14_test/
3. `perf(bench): add exec_bracket_loop_200 Criterion benchmark` — benches/exec_bench.rs
4. `docs(perf): correct §4.1 root cause and record [ builtin outcome` — performance.md + (optionally) TODO.md

Rationale: 4-commit split keeps cherry-pick easy, lets reviewers read the implementation separately from test infrastructure, and isolates the doc correction.

## 7. Out of Scope

- `-a`, `-o`, and complex `(` `)` nesting beyond the 3/4-operand forms. POSIX obsolescent.
- `<`, `>` bash lexicographic comparison.
- Arithmetic expression evaluation inside `test` (bash's `(( ... ))` is a separate syntax).
- Further §4.2 (function-call overhead) work — deferred to a follow-up spec.
- `pathname::expand` fast-path (§4.3) — separate P1 work, to be re-prioritized after this fix lands.

## 8. Risks & Mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Subtle POSIX semantic divergence in operand-count dispatch | Medium | E2E `POSIX_REF: 2.14 test` suite covers each dispatch arm; unit tests enumerate boundaries. |
| Filesystem permission edge cases (`-r`, `-w`, `-x`) flake in CI | Low | Use `std::fs::set_permissions` with explicit modes; run as non-root; `tempfile::tempdir` for isolation. |
| Regression in scripts that relied on external `test` side-effects (e.g., tracing via `strace`) | Very low | POSIX forbids such reliance; document in commit message. |
| `set -e` unexpectedly triggered by new builtin exit 2 | Low | Existing errexit-suppression handles `if`/`while`/`&&`/`||` contexts; add E2E test covering `if [ bad_op ]; then`. |
