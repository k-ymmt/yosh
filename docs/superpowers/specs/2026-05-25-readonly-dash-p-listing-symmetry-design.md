# readonly `-p` listing-trigger symmetry with export

**Date:** 2026-05-25
**Status:** Design approved
**TODO origin:** SP1 follow-up (TODO.md) —
"`readonly -- -p` triggers listing (rc=0) instead of treating `-p` as a
bad-identifier operand. Asymmetric with `export -- -p` (rc=1) ..."

## 1. Problem

`export` and `readonly` use different conditions to decide when `-p`
means "print the variable list" versus "an operand to validate":

```rust
// src/builtin/special.rs

// builtin_export (line 96)
if args.is_empty() || args[0] == "-p" {            // first position only

// builtin_readonly (line 188)
if args.is_empty() || args.iter().any(|a| a == "-p") {  // -p anywhere
```

Because `readonly` matches `-p` *anywhere* in `args`, `--` (end of
options) does not protect a following `-p`:

```sh
$ yosh -c 'export -- -p; echo rc=$?'
yosh: export: `-p': not a valid identifier
rc=1
$ yosh -c 'readonly -- -p; echo rc=$?'
rc=0                       # listing fired; -p was never validated
```

The `--` token is supposed to mark the end of options (XBD §12.2
Utility Syntax Guideline 10), so the trailing `-p` should be treated as
an operand and rejected as a bad identifier — exactly as `export` does.

This asymmetry was knowingly deferred when the `--` end-of-options
handling landed; see
`docs/superpowers/specs/2026-05-25-export-readonly-double-dash-design.md`
§3.3 and §4, which kept the `any(...)` condition and documented the
deviation. The test `readonly_double_dash_then_dash_p_triggers_listing`
locks down that (now-deviant) behavior. This spec resolves the
deferral.

## 2. Scope

**In:**

- `src/builtin/special.rs::builtin_readonly` — change the listing
  trigger to first-position-only so it mirrors `builtin_export`.
- Update / add unit tests in `src/builtin/special.rs`.
- Add symmetric E2E coverage for `readonly -- -p` and `export -- -p`.
- Resolve the deviation note in the prior spec; delete the TODO item.

**Out:**

- Any other `export` / `readonly` operand semantics (operand-drop on
  `export -p foo=v`, partial-assignment-on-error) — separate TODO items.
- `unset` `--` handling — already uses a flag-parse loop, unaffected.
- Full getopt-style option parsing — rejected as over-engineering for a
  single-option builtin (see §3, Approach 3).

## 3. Approaches considered

**Approach 1 (chosen): first-position-only listing trigger.**
Change `args.iter().any(|a| a == "-p")` to `args[0] == "-p"`. One-line
change; makes `readonly`'s listing logic structurally identical to
`export`. Side effect: `readonly foo -p` stops listing and instead
processes operands (assigns `foo` readonly, rejects `-p` with rc=1),
which is *more* POSIX-correct because options precede operands
(Guideline 9). This is the fix the TODO item recommends.

**Approach 2: only treat `-p` after `--` as an operand.** Scan for the
`--` position; trigger listing only when a `-p` precedes it. Preserves
`readonly foo -p` → listing but fixes `readonly -- -p`. Rejected: more
complex, and it does *not* achieve the goal of structural symmetry with
`export`.

**Approach 3: full getopt-style option parser.** Parse leading options
properly, accepting `-p` only as a leading option. Rejected:
over-engineering for a two-option builtin and inconsistent with the
existing minimalist implementation style.

## 4. Design

### 4.1 Core change (`src/builtin/special.rs::builtin_readonly`)

```rust
// before
// POSIX §2.14.11: "When invoked with no arguments or with the -p
// option, readonly shall write...". bash/dash treat -p as a listing
// trigger that suppresses any operand processing.
if args.is_empty() || args.iter().any(|a| a == "-p") {

// after
// POSIX §2.14.11: "When invoked with no arguments or with the -p
// option, readonly shall write...". Only `-p` in the first position
// triggers listing; `-p` after operands or after `--` (end of options,
// XBD §12.2 Guideline 10) is validated as a bad identifier. Mirrors
// builtin_export.
if args.is_empty() || args[0] == "-p" {
```

The rest of `builtin_readonly` (the `consume_end_of_options` call at
line 203 and the operand loop) is unchanged.

### 4.2 Behavior matrix (after fix)

| Input | Before | After | Matches export? |
|---|---|---|---|
| `readonly -- -p` | listing rc=0 | `-p` identifier error rc=1 | `export -- -p` ✓ |
| `readonly -p` | listing rc=0 | listing rc=0 (unchanged) | ✓ |
| `readonly -p --` | listing rc=0 | listing rc=0 (unchanged) | ✓ |
| `readonly foo -p` | listing rc=0 | `foo` set readonly + `-p` rc=1 | `export foo -p` ✓ |
| `readonly` (no args) | listing rc=0 | listing rc=0 (unchanged) | ✓ |

`readonly -p --` still lists because `args[0] == "-p"` matches first and
the early return fires before `consume_end_of_options` is reached —
identical to `export -p --`.

## 5. Tests

### 5.1 Unit tests (`src/builtin/special.rs` tests module)

- **Update** `readonly_double_dash_then_dash_p_triggers_listing` →
  rename to `readonly_double_dash_then_dash_p_is_invalid_identifier`;
  assert `status == 1`; replace the stale comment that described the
  `any(...)` deviation. Mirrors
  `export_double_dash_then_dash_p_is_invalid_identifier`.
- **Add** `readonly_p_then_double_dash_remains_listing` —
  `readonly -p --` → rc=0. Mirrors
  `export_p_then_double_dash_remains_listing` (listed in the prior
  spec's §5.1 but never implemented).
- **Add** `readonly_operand_then_dash_p_is_invalid_identifier` —
  `readonly foo -p` → rc=1 and `foo` is set readonly. Locks down the
  intended side effect of the change.
- Unchanged guards remain green: `readonly_dash_p_alone_remains_listing`,
  `readonly_double_dash_then_assignment_succeeds`,
  `readonly_double_dash_alone_is_noop_rc0`.

### 5.2 E2E tests (`e2e/posix_spec/4_special_builtin/`, 644 perms)

A symmetric pair documenting the end-of-options behavior at the
POSIX-spec level:

`readonly_dash_dash_dash_p.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14.11 readonly (XBD 12.2 Guideline 10)
# DESCRIPTION: readonly -- ends options; trailing -p is a bad identifier
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
readonly -- -p
```

`export_dash_dash_dash_p.sh`:
```sh
#!/bin/sh
# POSIX_REF: 2.14.9 export (XBD 12.2 Guideline 10)
# DESCRIPTION: export -- ends options; trailing -p is a bad identifier
# EXPECT_STDERR: export
# EXPECT_EXIT: 1
export -- -p
```

## 6. Documentation updates

- **This spec** records the resolution.
- **Prior spec** (`2026-05-25-export-readonly-double-dash-design.md`):
  annotate §3.3 and the §4 matrix row for `readonly -- -p` to note that
  the deviation is resolved by this follow-up (forward reference to this
  file).
- **TODO.md**: delete the SP1 follow-up item describing the
  `readonly -- -p` asymmetry (per the project convention of deleting
  completed items rather than marking `[x]`).

## 7. Verification

1. `cargo test special` — unit tests (updated + new) pass.
2. `cargo build` — debug build for the E2E runner.
3. `./e2e/run_tests.sh --filter=readonly` and
   `./e2e/run_tests.sh --filter=export` — new E2E pair passes, existing
   `*_dash_dash.sh` / `*_p_listing.sh` tests stay green.
