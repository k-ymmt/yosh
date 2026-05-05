# Large-File Responsibility Redesign — Umbrella

## Overview

Four files in `src/` exceed 1000 lines. Each carries multiple responsibilities that have accumulated over the project's growth. This umbrella design coordinates four independent sub-projects (SP1–SP4) that split each file along responsibility boundaries — not as mechanical moves but as deliberate redesigns that introduce small abstractions where the current code makes implicit state machines or boundary contracts hard to read.

The four target files are independent: SP1–SP4 can be executed in parallel branches without merge conflict.

## Goals

- Each production file ≤ 400 lines (one exception: `host/files.rs` ~440 lines, see SP1).
- Each split unit has one clearly-named responsibility — answerable in one sentence: *"what does it do?"*
- `mod.rs` is a thin facade: type definitions, public API signatures, and submodule delegation only. No implementation bodies.
- Public API (anything reachable from `yosh-plugin-*` crates or `bin/`) is preserved bit-for-bit.

## Non-Goals

- No public API breaking changes.
- No performance tuning. (If responsibility-split happens to help, that's a side effect, not the target.)
- No feature additions or bug fixes. Bugs spotted during split are filed as separate PRs.
- No work on files under 1000 lines (`plugin/mod.rs` 972, `lexer/word.rs` 923, etc. — out of scope).

## Common Principles

| Principle | Detail |
|---|---|
| `mod.rs` is a facade | Holds the canonical type definitions and public function signatures. Implementation bodies move to topical submodules. |
| Visibility is minimal | Symbols used only across new submodules become `pub(super)` / `pub(crate)`. `pub` reserved for items provably consumed by external crates or binaries. |
| Tests follow target | `#[cfg(test)] mod tests` blocks move with their target. Test helpers (e.g., `null_env_ctx`) live where most callers need them. |
| Public API preserved | External callers do not change. When a rename improves the internal name, use `pub use newname as oldname;` in `mod.rs` for compatibility. |
| Responsibility expressed in types | Where a split reveals an implicit state machine or contract, introduce a small struct/trait/enum to make it explicit. Don't add abstractions that aren't justified by the structure already present in the code. |

## Sub-Projects

| # | Target | Theme | Estimated PRs | Risk |
|---|---|---|---|---|
| SP1 | `src/plugin/host.rs` (1004 lines) | Capability-per-file split (`host/{variables,filesystem,io,files,commands}.rs`); introduce `with_env` helper to enforce metadata-contract structurally. | 1–2 | Low |
| SP2 | `src/env/jobs.rs` (1118 lines) | Split `JobTable` responsibilities into model / spec / notification / format / terminal modules; centralize notification state machine in predicates; introduce `Display for JobStatus`. | 2–3 | Medium |
| SP3 | `src/interactive/highlight_scanner.rs` (1594 lines) | Decouple scanner functions from `HighlightScanner` struct via a `ScanCtx<'a>` shared mutable state; one file per scan-mode group. | 2–3 | Medium |
| SP4 | `src/expand/mod.rs` (1230 lines) | Separate heredoc / pipeline / scan helpers / tilde into independent modules; reduce `expand_part_to_fields` via per-variant helpers. | 2–3 | High |

Recommended order: SP1 → SP2 → SP3 → SP4 (ascending risk). The four sub-projects are independent; they may be executed in parallel branches.

## Sub-Project Documents

Each sub-project has its own design document, written with full responsibility-redesign detail:

```
docs/superpowers/specs/
  2026-05-06-large-file-redesign-umbrella-design.md       (this file)
  2026-05-06-sp1-plugin-host-redesign-design.md
  2026-05-06-sp2-env-jobs-redesign-design.md
  2026-05-06-sp3-highlight-scanner-redesign-design.md
  2026-05-06-sp4-expand-mod-redesign-design.md
```

Each sub-project will produce its own implementation plan via the `writing-plans` skill, targeting the PR breakdown described in its design.

## Definition of Done (per sub-project)

Each SP is complete when **all** of the following hold on its target branch:

1. `cargo test` PASS (unit + integration).
2. `./e2e/run_tests.sh` PASS (full run, no filter).
3. `cargo bench --no-run` PASS — bench API not broken.
4. `cargo clippy --all-targets -- -D warnings` — only pre-existing violations remain (`src/plugin/mod.rs:98-99` `doc_lazy_continuation` is a known pre-existing issue and is out of scope for this umbrella).
5. `cargo fmt --check` PASS.
6. Each production file in the target ≤ 400 lines (with documented exceptions in the SP design).
7. README, CLAUDE.md, TODO.md references to the target file are still valid (verified via grep).
8. Public API names and signatures are preserved (no external-caller diffs).

## Cross-Cutting Notes

- **Multiple `impl T` blocks across files:** SP2 (and to a lesser extent SP1) relies on Rust's allowance of multiple `impl JobTable { ... }` blocks across submodules. This is a standard pattern; rustdoc collapses them into a single page.
- **Test helpers consolidation:** SP1 collapses 19 near-identical metadata-contract tests into one helper test plus 5 per-capability spot tests. Similar consolidations may surface in SP2/SP3 as redundancy becomes visible after the split.
- **Performance verification:** SP3 and SP4 modify code on the interactive hot path. Each provides a `cargo bench` step before and after the redesign-heavy PR; the threshold is ±5% from baseline. Greater regressions trigger redesign review.

## TODO.md Cleanup After Completion

Once all four SPs land, delete the following entries from TODO.md:

- "`src/plugin/host.rs` is now ~970 lines after the `commands:exec` addition. Consider splitting…" (consumed by SP1)
- "`skip_balanced_*` unterminated input tests" (consumed by SP4 PR-A)

Other TODO entries (KEYWORDS duplication, `JobTable::update_status` per-process tracking, `pre_exec`/`post_exec`/`on_cd` hook timeout extension, etc.) remain — they are out of scope here.

## Open Questions

None at this time. Each SP design is internally complete; the umbrella is a coordination document.
