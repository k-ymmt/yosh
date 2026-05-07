# SP4 — `src/expand/mod.rs` Responsibility Redesign

Part of the [Large-File Responsibility Redesign Umbrella](2026-05-06-large-file-redesign-umbrella-design.md).

## Current State

`src/expand/mod.rs` is 1230 lines. Six submodules already exist alongside it (`pattern.rs`, `param.rs`, `command_sub.rs`, `pathname.rs`, `field_split.rs`, `arith.rs`), so `mod.rs` is meant to be the central facade. In practice it is **the heaviest file in `expand/`** — five distinct concerns are inlined.

Production breakdown:

| Region | Lines | Notes |
|---|---|---|
| Public API | ~50 | `ExpandedField` type + `expand_word` / `expand_words` / `expand_word_to_string` |
| Heredoc | ~200 | `expand_heredoc_body` / `expand_heredoc_string` / `expand_heredoc_part` — POSIX §2.7.4 (no field-split, no pathname, no tilde) |
| Pipeline core | ~350 | `expand_word_to_fields` / `expand_part_to_fields` / `expand_param_to_fields` / `ifs_first_char` |
| Balanced-paren scanners | ~170 | `skip_balanced_parens` / `skip_balanced_braces` / `skip_balanced_double_parens` — lexical scan helpers |
| Tilde | ~45 | `expand_tilde_prefix` / `expand_tilde_user` |
| `ExpandedField` impl | ~80 | quoted-flag management |

## Proposed Structure

```
src/expand/
  mod.rs               — public API only: ExpandedField + expand_word/words/to_string  (~150 lines)
  pipeline.rs          — expand_word_to_fields, expand_part_to_fields,
                         expand_param_to_fields, ifs_first_char                        (~370 lines)
  heredoc.rs           — expand_heredoc_body/string/part                               (~220 lines)
  scan.rs              — skip_balanced_parens/braces/double_parens                     (~180 lines)
  tilde.rs             — expand_tilde_prefix, expand_tilde_user                        (~60 lines)
  pattern.rs           — (existing, untouched)
  param.rs             — (existing, untouched)
  command_sub.rs       — (existing, untouched)
  pathname.rs          — (existing, untouched)
  field_split.rs       — (existing, untouched)
  arith.rs             — (existing, untouched)
```

`mod.rs` becomes a true facade.

## Responsibility Redesign

### Heredoc as an Independent Module

POSIX §2.7.4 here-document expansion follows different rules than word expansion:

- No field splitting (output is always a single string).
- No pathname expansion.
- No tilde expansion.
- Quoted heredocs (`<<'EOF'`) suppress all expansion.
- Unquoted heredocs perform parameter, arithmetic, and command substitution only.

The current `expand_heredoc_*` family lives next to `expand_word_to_fields` in the same file, sharing the `expand_*` prefix as if it were a symmetric API. It is not — the pipelines are distinct.

Move heredoc to its own module with a focused public API:

```rust
// heredoc.rs
pub fn expand_body(env: &mut ShellEnv, parts: &[WordPart], quoted: bool) -> String { ... }

fn expand_string(env: &mut ShellEnv, s: &str) -> String { ... }
fn expand_part(env: &mut ShellEnv, part: &WordPart, out: &mut String) { ... }
```

For backward compatibility with existing callers (`exec/`, `lexer/heredoc.rs`), `mod.rs` re-exports under the original name:

```rust
pub use heredoc::expand_body as expand_heredoc_body;
```

### Pipeline Module — Per-Variant Helpers

`expand_part_to_fields` is an ~85-line `match` on 10 `WordPart` arms. Three of those arms (`EscapedLiteral`, `SingleQuoted`, `DollarSingleQuoted`) all funnel into `push_quoted` with no other behavior — collapse them into one helper. The remaining variants get their own helper. Result: 7 helpers in `pipeline.rs`:

```rust
fn expand_part_literal(...)         // WordPart::Literal — quoted-vs-unquoted split on in_double_quote
fn expand_part_quoted_literal(...)  // WordPart::EscapedLiteral / SingleQuoted / DollarSingleQuoted — always push_quoted
fn expand_part_double_quoted(...)   // WordPart::DoubleQuoted — recurses with in_double_quote=true
fn expand_part_tilde(...)           // WordPart::Tilde(Option<String>)
fn expand_part_parameter(...)       // WordPart::Parameter
fn expand_part_command_sub(...)     // WordPart::CommandSub
fn expand_part_arith_sub(...)       // WordPart::ArithSub
```

Each helper is 10–25 lines and writes into a shared `&mut Vec<ExpandedField>` accumulator. The `in_double_quote: bool` flag is threaded through every helper that has quoted/unquoted behavior, exactly as it is today. `expand_part_to_fields` becomes a ~25-line dispatch:

```rust
fn expand_part_to_fields(
    env: &mut ShellEnv,
    part: &WordPart,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) -> crate::error::Result<()> {
    match part {
        WordPart::Literal(s)             => expand_part_literal(s, fields, in_double_quote),
        WordPart::EscapedLiteral(s)
        | WordPart::SingleQuoted(s)
        | WordPart::DollarSingleQuoted(s) => expand_part_quoted_literal(s, fields),
        WordPart::DoubleQuoted(parts)    => expand_part_double_quoted(env, parts, fields)?,
        WordPart::Tilde(user)            => expand_part_tilde(env, user.as_deref(), fields),
        WordPart::Parameter(p)           => expand_part_parameter(env, p, fields, in_double_quote)?,
        WordPart::CommandSub(p)          => expand_part_command_sub(env, p, fields, in_double_quote),
        WordPart::ArithSub(e)            => expand_part_arith_sub(env, e, fields, in_double_quote)?,
    }
    Ok(())
}
```

`expand_part_to_fields` itself stays **private** (`fn`, not `pub(super)`): only `expand_word_to_fields` is called from `mod.rs` (via `expand_word` and `expand_word_to_string`), so only `expand_word_to_fields` needs `pub(super)` visibility.

The composition stages (Tilde / Parameter / Arithmetic / Command → Field-split → Pathname → Quote-removal) remain as today, called from the public `expand_word`. The structural change is keeping orchestration in `pipeline.rs` and leaving `mod.rs` as a thin caller.

### Scan Helpers Module + TODO Cleanup

`skip_balanced_parens`, `skip_balanced_braces`, `skip_balanced_double_parens` are lexical scanning helpers used by `heredoc.rs` (after the move) and `arith.rs`. They are not "expansion" — they are byte-level scanners with quote/escape awareness. Move to `scan.rs`.

TODO.md notes "`skip_balanced_*` unterminated input tests": all three return `bytes.len()` on unterminated input, but no test covers this. Add three tests in `scan.rs` (one per function) verifying the unterminated-input return value. After SP4 PR-A, delete the TODO entry.

### Tilde Module

`expand_tilde_prefix` and `expand_tilde_user` are already `pub(crate)` (referenced from `interactive/mod.rs` for ENV preprocessing per the v2 spec). Move to `tilde.rs` keeping the same visibility.

## `mod.rs` Final Shape

```rust
mod pipeline;
mod heredoc;
mod scan;
mod tilde;
pub mod pattern;
pub mod param;
pub mod command_sub;
pub mod pathname;
pub mod field_split;
pub mod arith;

pub(crate) use scan::{skip_balanced_braces, skip_balanced_double_parens, skip_balanced_parens};
pub(crate) use tilde::{expand_tilde_prefix, expand_tilde_user};
pub use heredoc::expand_body as expand_heredoc_body;

pub struct ExpandedField { /* canonical type definition */ }
impl ExpandedField { ... }
impl Default for ExpandedField { ... }

pub fn expand_word(env: &mut ShellEnv, word: &Word) -> crate::error::Result<Vec<String>> { ... }
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> crate::error::Result<Vec<String>> { ... }
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> crate::error::Result<String> { ... }
```

`ExpandedField` stays in `mod.rs` because it is referenced by `pipeline.rs`, `field_split.rs`, and downstream consumers — moving it into a submodule would create awkward cross-module imports for what is the central data type of the expand pipeline.

## Test Reorganization

| Existing Tests | New Location |
|---|---|
| `expand_word` / `expand_words` / `expand_word_to_string` API tests | `mod.rs` |
| `expand_word_to_fields` / `expand_part_to_fields` tests | `pipeline.rs` |
| `expand_heredoc_*` tests | `heredoc.rs` |
| `skip_balanced_*` tests + **new unterminated-input tests** | `scan.rs` |
| `expand_tilde_*` tests | `tilde.rs` |

## PR Breakdown

1. **PR-A — Scan + tilde extraction.** Lowest dependency surface. Move `skip_balanced_*` to `scan.rs` (with `pub(crate) use` re-export from `mod.rs`). Move `expand_tilde_*` to `tilde.rs` (with `pub(crate) use` re-export). Add three unterminated-input tests in `scan.rs`. Delete the corresponding TODO entry.
2. **PR-B — Heredoc extraction.** Move `expand_heredoc_*` to `heredoc.rs`, renaming `expand_heredoc_body` → `expand_body` internally. Re-export from `mod.rs` as `pub use heredoc::expand_body as expand_heredoc_body;`. Move heredoc tests.
3. **PR-C — Pipeline extraction + redesign.** Move `expand_word_to_fields`, `expand_part_to_fields`, `expand_param_to_fields`, `ifs_first_char` to `pipeline.rs`. Decompose `expand_part_to_fields` into per-variant private helpers. `mod.rs` becomes pure facade. Move pipeline tests.

Order matters: PR-C builds on PR-A/PR-B (uses `scan::skip_balanced_*` and references the heredoc module). The PRs must merge in order.

## Risks

- **`expand` is the central runtime path** — any regression cascades to every test in the suite. After each PR, run `./e2e/run_tests.sh` with no filter and confirm green.
- **Per-variant decomposition must preserve behavior bit-for-bit** — each helper's output (including `ExpandedField.quoted` flag transitions) must equal the original `match` arm output. Reviewers verify by reading helpers against the pre-PR-C source.
- **`expand_heredoc_body` rename** is internal-only; the external name is preserved by `pub use ... as ...`. Callers (`exec/`, `lexer/heredoc.rs`) require zero diff.
- **Performance** — `expand_word` is a hot path. Run `cargo bench --bench expand_bench` before and after PR-C; threshold ±5% of PR-B baseline.
- **`ExpandedField` placement** — keeping the type in `mod.rs` (rather than moving to `pipeline.rs`) is a deliberate design call: it is the shared interface type between `pipeline.rs`, `field_split.rs`, and downstream code. Centralizing it in `mod.rs` minimizes cross-module dependency churn.

## Public API Compatibility

Preserved signatures and paths:

- `crate::expand::expand_word`
- `crate::expand::expand_words`
- `crate::expand::expand_word_to_string`
- `crate::expand::expand_heredoc_body`
- `crate::expand::ExpandedField`
- `crate::expand::expand_tilde_prefix`
- `crate::expand::expand_tilde_user`
- `crate::expand::skip_balanced_parens`
- `crate::expand::skip_balanced_braces`
- `crate::expand::skip_balanced_double_parens`

The existing `pub mod pattern/param/command_sub/pathname/field_split/arith` declarations remain as-is.

## Definition of Done

- `cargo test` PASS.
- `./e2e/run_tests.sh` PASS (full, no filter).
- `cargo bench --no-run` PASS, and `expand_bench` shows expand throughput within ±5% of PR-B baseline.
- Each production file ≤ 370 lines.
- `mod.rs` is ~150 lines of facade.
- TODO.md entry "`skip_balanced_*` unterminated input tests" is removed (resolved by PR-A).
- Public-API compatibility verified by zero-diff in callers (`grep -r "crate::expand::" src/ | sort` before and after).
