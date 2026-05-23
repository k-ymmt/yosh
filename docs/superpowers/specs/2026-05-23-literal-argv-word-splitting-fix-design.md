# Literal Argv Word-Splitting Fix Design

Date: 2026-05-23
Status: Approved (brainstorming)
TODO entry: `TODO.md` §"SP3 follow-ups (non-blocking)" #1

## 1. Problem

POSIX XCU §2.6.5 (Field Splitting) restricts field splitting to the
results of parameter expansion, command substitution, and arithmetic
expansion — never to literal text. yosh currently violates this by
treating every unquoted byte (including bytes from `WordPart::Literal`)
as splittable, so a literal argv token like `a::b` is split by an
IFS-delimiter into multiple fields.

### Reproduction

```sh
$ IFS=:; printf "[%s]\n" a::b
# dash / bash --posix:
[a::b]

# yosh (HEAD):
[a]
[]
[b]
```

The `printf "a::b\n" | { read x y z; ... }` variant works correctly
because the IFS character is consumed by `read`, not by the expander on
the literal — confirming the bug is in the expand pipeline, not in
field-splitting downstream.

## 2. Root Cause

`ExpandedField` (`src/expand/mod.rs`) tracks per-byte attributes with a
single `quoted_mask: Vec<u64>`. The mask conflates two distinct POSIX
notions:

- **Protected from field splitting** — bytes that must not be split on
  IFS (quoted bytes, literal bytes).
- **Protected from pathname expansion** — bytes whose `*`, `?`, `[`
  must not be treated as glob metacharacters (quoted bytes only;
  literal `*` must still glob).

The two notions overlap on truly-quoted bytes but diverge on literal
bytes. yosh's two-state classification — `push_quoted` (both protected)
vs `push_unquoted` (neither protected) — forces literal bytes into the
`push_unquoted` bucket, breaking the field-splitting invariant.

The required POSIX classification is three-state:

| Origin                        | Field splitting | Pathname expansion | `was_quoted` |
|-------------------------------|----------------:|-------------------:|-------------:|
| `''` `""` `\x` Tilde          |       protected |          protected |         true |
| `Literal` (currently broken)  |       protected |        **subject** |    unchanged |
| `$var` `$(...)` `$((...))`    |         subject |            subject |    unchanged |

## 3. Approach

Three approaches were considered (see brainstorming transcript for
detail):

- **A. Two independent masks** — `split_protected_mask` and
  `glob_protected_mask` are tracked separately. Each predicate reads
  the mask matching its own responsibility.
- **B. One mask + literal flag** — keep `quoted_mask` for split
  protection, add `literal_mask` that pathname consults as
  `quoted & !literal`.
- **C. Pre-split at literal/expansion boundaries** — restructure
  `expand_word_to_fields` so that literal and expansion regions live
  in separate `ExpandedField`s and field splitting visits only
  expansion regions.

**Decision: Approach A.**

Performance is indistinguishable across the three (memory +1 `Vec<u64>`
in A and B; C may increase the field count). The deciding factors are
clarity and future-proofing: Approach A maps one mask to one
responsibility with no hidden invariants, while Approach B carries an
implicit `literal_mask ⊆ quoted_mask` constraint and Approach C does
not generalise to future per-byte attributes (e.g. brace-expansion
gating, locale-dependent splitting).

## 4. Data Model

```rust
pub struct ExpandedField {
    pub value: String,
    /// bit set = byte must not be split on IFS
    split_protected_mask: Vec<u64>,
    /// bit set = byte must not be treated as a glob metacharacter
    glob_protected_mask: Vec<u64>,
    /// POSIX: a quoted context was applied; preserve zero-length field
    pub was_quoted: bool,
}
```

### Public push API

| Method                            | split_protected | glob_protected | was_quoted |
|-----------------------------------|----------------:|---------------:|-----------:|
| `push_quoted(s)`                  |               ✓ |              ✓ |       true |
| `push_literal(s)` *(new)*         |               ✓ |              ✗ |  unchanged |
| `push_expanded(s)` *(was unquoted)* |             ✗ |              ✗ |  unchanged |

### Public query API

| Method                       | Reads                  | Used by             |
|------------------------------|------------------------|---------------------|
| `is_split_protected(i)`      | split_protected_mask   | `field_split.rs`    |
| `is_glob_protected(i)`       | glob_protected_mask    | `pathname.rs`       |

### Removed

- `push_unquoted` (renamed to `push_expanded` at every call site)
- `is_quoted` (replaced by the two new predicates at every call site)

### Constructors

- `ExpandedField::new()` — both masks empty, `was_quoted=false`.
- `ExpandedField::all_quoted(value)` — both masks set to `u64::MAX`
  over the full byte range; `was_quoted=false` (unchanged from today,
  matches `pathname::expand`'s use for glob match results).

### Invariants

None. The two masks are independent. The combination
`(split=false, glob=true)` is not produced by any push method but is
not rejected — should it ever occur (e.g. through future direct mask
manipulation), `append_char` in field_split routes it through
`push_expanded` defensively. No `debug_assert!` is added; this is a
helpful property, not a load-bearing invariant.

## 5. Components Changed

### `src/expand/mod.rs` (data model)

- Replace `quoted_mask` with `split_protected_mask` + `glob_protected_mask`.
- Add helper `fn set_mask_range(mask: &mut Vec<u64>, start, len)` that
  resizes and OR-sets bits in one mask. `push_quoted` calls it on
  both, `push_literal` on `split_protected_mask` only, `push_expanded`
  on neither.
- Implement `is_split_protected(i)` and `is_glob_protected(i)`.
- Update `all_quoted` to mark both masks.
- Update `Default` / `new()` accordingly.
- Migrate the existing test module: rename `is_quoted` /
  `push_unquoted` call sites and add tests for `push_literal` and the
  two predicates.

### `src/expand/pipeline.rs` (the actual fix is here)

The bug-fix line:

```rust
// expand_part_literal, unquoted branch (the only behavioural change)
- fields.last_mut().unwrap().push_unquoted(s);
+ fields.last_mut().unwrap().push_literal(s);
```

All other touches are API renames with unchanged semantics:

- `expand_part_command_sub`, `expand_part_arith_sub`, unquoted-`$@`
  per-param push: `push_unquoted` → `push_expanded`.
- `expand_part_quoted_literal`, `expand_part_tilde`,
  double-quoted-`Literal`, `$@`/`$*` inside `""`: `push_quoted`
  unchanged.

### `src/expand/field_split.rs` (predicate swap + push routing)

- `needs_splitting`: `is_quoted` → `is_split_protected`.
- `split_field`: `field.is_quoted(i)` → `field.is_split_protected(i)`.
- `append_char`: choose the push method based on the byte's
  `(is_split_protected, is_glob_protected)` pair so that literal bytes
  that survive splitting retain their literal-ness (still glob-able in
  a later `pathname::expand` pass — currently dead code given the
  pipeline order, but correct in principle and cheap).

  ```rust
  fn append_char(dest: &mut ExpandedField, source: &ExpandedField, i: usize) -> usize {
      let ch_len = source.value[i..].chars().next().expect("char boundary").len_utf8();
      let slice = &source.value[i..i + ch_len];
      let split_p = source.is_split_protected(i);
      let glob_p = source.is_glob_protected(i);
      match (split_p, glob_p) {
          (true, true)   => dest.push_quoted(slice),
          (true, false)  => dest.push_literal(slice),
          (false, false) => dest.push_expanded(slice),
          (false, true)  => dest.push_expanded(slice), // defensive; not produced today
      }
      ch_len
  }
  ```

### `src/expand/pathname.rs` (predicate swap)

- `has_unquoted_glob_chars`: `field.is_quoted(i)` → `field.is_glob_protected(i)`.
- `all_quoted(m)` usage unchanged (glob match results are both
  protected — they must not be re-split or re-globbed).

### Not changed

- `src/exec/`, `src/builtin/`, `src/parser/` — `expand_word` /
  `expand_words` / `expand_word_to_string` keep their signatures and
  return types.

## 6. Data Flow

```
Word.parts
  ↓ expand_part_to_fields (pipeline.rs)
  ├─ Literal               → push_literal   (split=✓, glob=✗)  ← only behavioural change
  ├─ EscapedLiteral        → push_quoted    (split=✓, glob=✓)
  ├─ SingleQuoted          → push_quoted    (split=✓, glob=✓)
  ├─ DollarSingleQuoted    → push_quoted    (split=✓, glob=✓)
  ├─ DoubleQuoted          → was_quoted=true, recurse with in_double_quote=true
  │    └─ Literal          → push_quoted    (split=✓, glob=✓)
  ├─ Tilde                 → push_quoted    (split=✓, glob=✓)
  ├─ Parameter $var        → push_expanded  (split=✗, glob=✗)
  ├─ CommandSub $(...)     → push_expanded  (split=✗, glob=✗)
  └─ ArithSub $((...))     → push_expanded  (split=✗, glob=✗)
  ↓ field_split::split    (uses is_split_protected)
  ↓ pathname::expand      (uses is_glob_protected)
  ↓ quote removal + empty-field filter (was_quoted unchanged)
```

### Behaviour matrix

| Input                                | Expected      | yosh HEAD     | yosh post-fix |
|--------------------------------------|--------------:|--------------:|--------------:|
| `IFS=:; printf "[%s]\n" a::b`        |     `[a::b]`  | `[a][][b]` ✗  |    `[a::b]` ✓ |
| `IFS=:; v=a::b; printf "[%s]\n" $v`  |    `[a][][b]` |   `[a][][b]` ✓|   `[a][][b]` ✓|
| `echo *.rs`                          | matching `.rs`|matching `.rs`✓|matching `.rs`✓|
| `echo "*.rs"`                        |       `*.rs`  |       `*.rs` ✓|       `*.rs` ✓|
| `IFS=:; echo a::b $v` with `v=x:y`   | `a::b x y`    | `a  b x y` ✗  |    `a::b x y`✓|

Literal-text glob expansion is preserved; only literal-text field
splitting changes.

## 7. Error Handling

No new error conditions. `expand_word` retains its `Result` shape; the
sole existing error site (`ExpansionErrorKind::InvalidArithmetic`) is
untouched. Push methods are infallible.

## 8. Testing

### Unit (`src/expand/`)

`mod.rs::tests`:
- `push_literal_marks_split_protected_only`
- `push_quoted_marks_both_and_was_quoted`
- `push_expanded_marks_neither`
- `mixed_push_per_byte_independence`
- `all_quoted_marks_both`

`field_split.rs::tests`:
- `literal_colon_not_split` — direct bug reproduction at unit level
- `literal_then_expansion_split_only_in_expansion_region`
- `literal_then_expansion_then_literal_round_trip`
- Existing `test_double_colon_empty_field` retained, updated to use
  `push_expanded` (semantics unchanged).

`pathname.rs::tests`:
- `literal_asterisk_still_globs`
- `quoted_asterisk_does_not_glob` (regression pin)

### E2E (`e2e/posix_spec/2_shell/2_6_5_field_splitting/`)

Create directory if absent. Tests:
- `literal_argv_not_split.sh` — `IFS=:; echo a::b` → `a::b`
- `literal_with_ifs_nonwhite_consecutive.sh` — `IFS=:; printf "[%s]\n" a:b:c`
- `literal_then_var_split_correctly.sh` — mixed literal/expansion
- `literal_with_glob_metachar_still_globs.sh` — `echo *.toml` (must
  match `Cargo.toml`; document the dependency)

### Regression

- `cargo test --lib` (expand module + everything else)
- `cargo test` (integration tests)
- `./e2e/run_tests.sh` (full E2E; XFAIL count must be unchanged or
  reduce; no new failures)
- Manual smoke: TODO repro plus `read` round-trip
  (`printf "a::b\n" | { read x y z; echo "x=$x y=$y z=$z"; }`)

## 9. Out of Scope

- Multi-byte (non-ASCII) IFS character matching — separate TODO entry
  in "Future: Code Quality Improvements", deferred from 2026-04-21.
- `${var:=value}` PATH-cache invalidation paths (SP2 follow-up #1) —
  unrelated to the literal-vs-expansion classification.
- Pathname expansion semantics — no change to glob matching beyond the
  predicate-source swap.

## 10. Risks

- **Field_split test corpus changes**: the existing
  `test_double_colon_empty_field` keeps its meaning (it uses
  `push_unquoted` → renamed `push_expanded`, both treat the field as
  splittable). Rename-only diff; semantics frozen.
- **`append_char` push routing**: the four-case match is new logic.
  The `(false, true)` branch is defensive (no current push method
  produces it). A `// not produced by current push API; routed as
  expanded for forward compatibility` comment at that arm is
  sufficient; no test is required since the input cannot be
  constructed through the public API.
- **Downstream consumers**: `expand_word` / `expand_words` /
  `expand_word_to_string` keep their signatures. No `src/exec/` or
  `src/builtin/` change.
