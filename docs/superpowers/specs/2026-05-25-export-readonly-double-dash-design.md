# export / readonly `--` end-of-options regression fix

**Spec date:** 2026-05-25
**Status:** Approved (pending implementation)
**TODO origin:** SP1 follow-ups — "`export -- foo=v` and `readonly -- foo=v` now report `--` as not a valid identifier (visible regression after SP1 G2's strict gate)."

## 1. Problem

After SP1 G2 tightened identifier validation in `export` / `readonly`, the
POSIX XBD §12.2 Utility Syntax Guideline 10 convention — `--` marks the end
of options — stopped being honored by these two builtins. Reproduction at
HEAD:

```
$ yosh -c 'export -- foo=v; echo "rc=$?"'
yosh: export: `--': not a valid identifier
rc=1

$ yosh -c 'readonly -- bar=ok; echo "rc=$?"; echo "bar=$bar"'
yosh: readonly: `--': not a valid identifier
rc=1
bar=ok
```

`bar=ok` lands anyway because the operand loop calls `continue` on the bad
identifier and proceeds to `bar=ok` on the next iteration, which is itself a
secondary issue (partial assignment on error) — but the user-visible failure
is the unwanted diagnostic and rc=1.

`builtin_unset` (same file) already implements `--` skip inline as part of
its `-fv` flag-parse loop and is not regressed.

Existing e2e tests `e2e/posix_spec/4_special_builtin/export_dash_dash.sh`
and `readonly_dash_dash.sh` are false positives — they assert the side-effect
of the next operand being processed and only the *last* command's exit
status, which masks the rc=1 from the failing operand. They must be
tightened as part of this fix.

## 2. Scope

In:
- `src/builtin/special.rs::builtin_export` — accept `--` as end-of-options
- `src/builtin/special.rs::builtin_readonly` — accept `--` as end-of-options
- `src/builtin/special.rs::builtin_unset` — refactor existing inline `--`
  handling to call the shared helper (behavior preserved)
- New private helper `consume_end_of_options(args, idx) -> usize`
- Tighten 2 existing e2e tests to detect rc!=0 from the builtin itself
- New unit tests in `src/builtin/special.rs` (`#[cfg(test)] mod tests`)
- New e2e `unset_dash_dash.sh` to lock down unset's behavior after refactor

Out:
- `export -p foo=v` operand-drop fix (separate TODO item)
- `command`, `local`, other builtins (separate TODO if needed; user
  explicitly scoped this fix to 3 builtins)
- SP1 G2 strict-identifier gate redesign (orthogonal)
- Partial-assignment-on-error semantics in export/readonly operand loops

## 3. Design

### 3.1 Helper

```rust
// src/builtin/special.rs (private)
//
// POSIX XBD §12.2 Utility Syntax Guideline 10: `--` marks end of options.
// Shared by export / readonly / unset.
fn consume_end_of_options(args: &[String], idx: usize) -> usize {
    if args.get(idx).map(String::as_str) == Some("--") {
        idx + 1
    } else {
        idx
    }
}
```

Pure function, no side effects, easy to unit-test.

### 3.2 `builtin_export` changes

After the existing `args.is_empty() || args[0] == "-p"` listing branch,
compute the operand start and iterate from there:

```rust
let start = consume_end_of_options(args, 0);
for arg in &args[start..] {
    // existing per-operand logic unchanged
}
```

### 3.3 `builtin_readonly` changes

Mirror of 3.2: insert `consume_end_of_options` between the listing branch
and the operand loop. Note that the listing condition is currently
`args.is_empty() || args.iter().any(|a| a == "-p")` — once Guideline 10 is
applied, only `-p` *before* `--` should trigger listing. The simplest and
most bash-compatible reading is to keep the existing listing condition
unchanged (a `-p` *after* `--` is then treated as a bad identifier, matching
bash). No behavior change for valid inputs.

### 3.4 `builtin_unset` changes

Existing inline branch in the flag-parse loop:
```rust
if arg == "--" {
    idx += 1;
    break;
}
```
is replaced by:
```rust
if arg == "--" {
    idx = consume_end_of_options(args, idx);
    break;
}
```
Semantically identical (helper returns `idx + 1` when `args[idx] == "--"`).
This is a pure refactor — no behavior change — and ensures all three
builtins route through the same helper.

## 4. Behavior matrix

| Input | Expected (POSIX + bash/dash) | Pre-fix yosh | Post-fix yosh |
|---|---|---|---|
| `export -- foo=v` | export foo=v, rc=0 | `--` identifier error rc=1 (foo still assigned) | export foo=v, rc=0 |
| `export --` | rc=0, no-op | `--` identifier error rc=1 | rc=0, no-op |
| `export -- -p` | rc=1, `-p` identifier error | `--` identifier error, `-p` identifier error, rc=1 | `-p` identifier error rc=1 |
| `export -p` | listing rc=0 | unchanged | unchanged |
| `export -p --` | listing rc=0 (listing branch fires on `args[0]=="-p"`) | listing rc=0 | listing rc=0 |
| `readonly -- foo=v` | foo readonly, rc=0 | `--` identifier error rc=1 (foo still assigned + readonly) | foo readonly, rc=0 |
| `unset -- foo` | foo unset, rc=0 | already works (inline `--` handling) | works (refactored) |
| `unset -f -- foo` | function foo unset, rc=0 | already works | works |
| `unset -v -- -f` | rc=1, `-f` identifier error | already works | works |

## 5. Tests

### 5.1 Unit tests (`src/builtin/special.rs`)

If a `#[cfg(test)] mod tests` block does not yet exist in this file, create
one. Add 11 tests covering the behavior matrix:

```
test_export_double_dash_then_assignment
test_export_double_dash_alone
test_export_double_dash_then_invalid_operand
test_export_p_then_double_dash_listing

test_readonly_double_dash_then_assignment
test_readonly_double_dash_alone
test_readonly_double_dash_then_invalid_operand
test_readonly_p_then_double_dash_listing

test_unset_double_dash_preserves_existing_behavior
test_unset_f_then_double_dash
test_unset_v_double_dash_invalid_operand
```

Each test 5–15 lines. Use the existing test scaffolding pattern in the file
(or in sibling `src/builtin/*.rs` files) — instantiate `ShellEnv`, call
the builtin directly, assert on `Result<i32, _>` and on `env.vars` state.

### 5.2 E2E tests (`e2e/posix_spec/4_special_builtin/`)

**Tighten existing 2 tests** to detect rc!=0 from the builtin itself
(currently false positives). The pattern is `<builtin> ... || exit 99` so
a regression to rc=1 surfaces as both EXPECT_OUTPUT mismatch (no output
from the follow-up command) and EXPECT_EXIT mismatch (99 != 0):

`export_dash_dash.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export (XBD 12.2 Guideline 10)
# DESCRIPTION: export -- treats following operands as names; -- itself is consumed
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
export -- foo=hi || exit 99
sh -c 'echo "$foo"'
```

`readonly_dash_dash.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly (XBD 12.2 Guideline 10)
# DESCRIPTION: readonly -- treats following operands as names; -- itself is consumed
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
readonly -- foo=ok || exit 99
echo "$foo"
```

**Add 1 new test**:

`unset_dash_dash.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14.18 unset (XBD 12.2 Guideline 10)
# DESCRIPTION: unset honors -- after flag parsing
# EXPECT_OUTPUT: empty
# EXPECT_EXIT: 0
m=set
unset -- m || exit 99
echo "${m-empty}"
```

All 3 files: `chmod 644`.

## 6. Verification

```bash
cargo build
cargo test --lib builtin::special
cargo test                                  # full unit + integration
./e2e/run_tests.sh --filter=dash_dash       # focused
./e2e/run_tests.sh                          # full e2e regression
```

Tightened e2e tests must fail at `git stash` (pre-fix state) and pass after
the implementation lands.

## 7. Risk

- **Unset refactor breaks existing behavior**: low. Helper returns
  `idx + 1` iff `args[idx] == "--"`, which is identical to the inline
  `idx += 1; break;`. `test_unset_double_dash_preserves_existing_behavior`
  is the regression guard.
- **Listing branch interaction**: low. `export -p`/`readonly -p` listing
  fires on `args[0]=="-p"` (or `args.iter().any(...)` for readonly) before
  the helper is reached.
- **e2e tightening exposes other regressions**: low-medium. If tightening
  surfaces unrelated rc!=0 paths in `export`/`readonly` operand handling,
  document them as separate TODO items and keep this fix focused.

## 8. Rollback

Single commit, ~30 LoC delta. `git revert` is sufficient.

## 9. Follow-ups not addressed here

- `export -p foo=v` operand silent drop (SP1 follow-up, separate TODO)
- Partial-assignment-on-error in export/readonly operand loops (bash
  matches; not POSIX-mandated)
- Guideline 10 sweep across other builtins (`command`, etc.) — only if
  surfaced by user need
