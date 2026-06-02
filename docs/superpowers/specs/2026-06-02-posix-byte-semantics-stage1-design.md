# POSIX Byte Semantics Stage 1 Design

## Context

`TODO.md` tracks full POSIX byte transparency as a future item. Today yosh
intentionally stores shell source, words, variables, positional parameters, and
most expansion results as UTF-8 `String` values. That means invalid UTF-8 shell
input, argv, environment values, and paths cannot be preserved end to end.

The existing expansion pipeline already has one useful foundation:
`ExpandedField` tracks split and glob protection with one bit per byte. This
stage builds on that byte-oriented metadata without attempting a repo-wide
conversion of the parser, AST, environment store, builtins, and plugin APIs.

## Goals

- Make expansion-field value handling more explicitly byte-oriented.
- Reduce direct dependence on `ExpandedField.value: String` at processing
  boundaries where byte storage will matter later.
- Preserve current UTF-8 behavior and performance for existing scripts.
- Add focused tests that lock in byte-index protection behavior for multi-byte
  UTF-8 content.
- Update `TODO.md` so the broad future item is decomposed into completed stage-1
  work and explicit remaining work.

## Non-Goals

- Accept invalid UTF-8 shell source from stdin, `-c`, or script files.
- Preserve invalid UTF-8 command-line arguments or environment values.
- Convert `WordPart`, `ParamExpr`, `VarStore`, aliases, traps, functions, or
  plugin APIs to raw byte storage.
- Change locale-dependent character semantics for pattern ranges, character
  classes, or multi-byte IFS.
- Change user-visible output for valid UTF-8 scripts.

## Architecture

Keep parser and AST inputs as UTF-8 `String` for this stage. Add a narrow
byte-oriented API around `ExpandedField` and use it inside expansion consumers
instead of reaching into the string directly when byte behavior is relevant.

The intended shape is:

- `ExpandedField` remains the expansion pipeline carrier.
- It exposes explicit byte accessors, byte length, and conversion helpers for
  callers that need raw bytes or final UTF-8 strings.
- Existing split/glob protection masks remain packed `Vec<u64>` bitsets indexed
  by byte offset.
- Field splitting and pathname expansion continue to operate on byte offsets.
- Final public APIs can still return `String` until the AST/env boundary is
  migrated in a later stage.

This keeps stage 1 small and useful: it creates better internal boundaries for
future `Vec<u8>` or `OsString` storage without forcing a broad type migration
now.

## Components

### ExpandedField

Add methods that make byte semantics explicit:

- `as_bytes(&self) -> &[u8]`
- `byte_len(&self) -> usize`
- `into_string(self) -> String`
- small append helpers that keep mask updates tied to byte length

Callers that inspect offsets should use these APIs rather than `value.len()` or
`value.as_bytes()` directly. The public `value` field can remain for stage 1 if
changing it would create unnecessary churn, but new code should prefer methods.

### Field Splitting

Keep field splitting byte-based. Add tests showing that multi-byte UTF-8 bytes
do not corrupt split protection:

- quoted multi-byte content is protected across all bytes
- literal multi-byte content is split-protected but still glob-subject
- expanded multi-byte content remains split-subject

These tests verify the current byte-mask model and protect future refactors.

### Pathname Expansion

Keep valid UTF-8 glob behavior unchanged. Add or tighten tests that glob
protection is tracked per byte, especially where a multi-byte character sits
next to `*`, `?`, or bracket syntax. Stage 1 does not make non-UTF-8 filenames
fully matchable.

### Exec And Redirection Boundaries

Introduce a small helper for converting final expanded fields to `CString` for
external command execution. It should centralize NUL rejection and document that
the input is still UTF-8 in stage 1. This avoids scattering assumptions and
gives the later byte-storage migration one boundary to change.

Redirection path handling can keep accepting `String` in stage 1, but tests
should preserve current behavior around invalid NULs and valid UTF-8 paths.

## Error Handling

Stage 1 keeps existing errors for invalid command names, invalid arguments, and
I/O failures. The new boundary helper should return enough context for callers
to preserve current messages:

- command name with interior NUL: `yosh: <cmd>: invalid command name`
- argument with interior NUL: `yosh: <arg>: invalid argument`

No new syntax errors should be introduced for valid UTF-8 scripts.

## Performance

The stage should avoid converting every field to a new byte vector. Byte access
should borrow from the existing string buffer. Mask updates should continue to
use byte lengths and packed bitsets. Any helper introduced at exec boundaries
may allocate `CString`s as the code already does today.

Tests should cover behavior, not benchmark internals. If an implementation
needs a broader data-structure change, it should be deferred to a later stage.

## Testing

Run focused unit tests around expansion first:

- `cargo test expand::`
- targeted unit tests in `src/expand/mod.rs`, `src/expand/field_split.rs`, and
  `src/expand/pathname.rs` for byte-mask behavior

Then run broader regression checks:

- `cargo test`
- `cargo build`

If full `cargo test` is too slow or fails due to unrelated dirty worktree
changes, record the exact failure and the focused tests that passed.

## TODO Update

After implementation, replace the single broad TODO entry with:

- a note that stage 1 established byte-oriented expansion-field boundaries
- remaining work for invalid UTF-8 source input
- remaining work for argv/env/variable storage as bytes or `OsString`
- remaining work for paths and process boundaries
- remaining work for plugin API byte semantics

Do not mark the entire POSIX Byte Semantics item as done until invalid UTF-8
input, argv, paths, and environment values are supported end to end.
