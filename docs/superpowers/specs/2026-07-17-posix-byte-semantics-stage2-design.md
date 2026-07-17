# POSIX Byte Semantics Stage 2 Design — Escaped-Byte Encoding

## Context

Stage 1 (`2026-06-02-posix-byte-semantics-stage1-design.md`) made expansion
fields byte-mask oriented but left every value carrier (`WordPart`,
`ExpandedField.value`, `VarStore`, positional params, argv) as UTF-8 `String`.
`TODO.md` kept the umbrella item open until invalid UTF-8 data is preserved
end to end, and sketched a `Vec<u8>`/`OsString` type migration as the path.

A survey (2026-07-17) measured that migration's blast radius: `env.vars.get`
(86 sites) + `set` (77 sites), every builtin's `Vec<String>` argv, ~50 AST
test constructions, all `&str` pattern/param helpers. A full byte-buffer
type migration would touch thousands of sites across all 39k lines and
destabilize the interactive and plugin layers.

## Decision: internal escaped-byte encoding instead of a type migration

Adopt the approach proven by fish shell (private-use-area escaping) and
Python (PEP 383 surrogateescape): keep `String` as the universal carrier and
make it *losslessly* representable for arbitrary bytes.

- Each byte that cannot be decoded as UTF-8 is mapped, at an ingress
  boundary, to the private codepoint `U+10FE00 + byte` (Plane-16 PUA-B,
  `U+10FE80..=U+10FEFF` used for `0x80..=0xFF`).
- At egress boundaries the escape codepoints are mapped back to their raw
  bytes; all other chars re-encode as normal UTF-8.
- Injectivity: if ingress data contains a *real* codepoint inside the escape
  range (only possible from valid UTF-8), each of its UTF-8 bytes is escaped
  individually, so decode restores the identical byte sequence. Encode is a
  bijection between byte strings and their canonical encoded forms.

Everything between ingress and egress — lexer, AST, expansion, variables,
positional params, pattern matching, field splitting, builtins — continues
to operate on `String` unchanged. An escaped byte is one `char`, which gives
bash-compatible behavior for `${#var}` (an invalid byte counts as 1) and for
`?` in patterns (matches a single invalid byte).

This supersedes the byte-buffer migration plan recorded in `TODO.md`. The
observable acceptance criterion is unchanged: invalid UTF-8 shell input,
argv, paths, and environment values are preserved end to end.

## Core module: `src/byteenc.rs`

- `pub const ESCAPE_BASE: u32 = 0x10FE00;`
- `pub fn encode_bytes(bytes: &[u8]) -> Cow<str>` — decode maximal valid
  UTF-8 chunks; escape invalid bytes and any real chars in the escape range.
  Borrows when the input is valid UTF-8 with no in-range chars (fast path).
- `pub fn decode_bytes(s: &str) -> Cow<[u8]>` — inverse; borrows when no
  escape char is present (fast path: no `0xF4` byte in the UTF-8).
- `pub fn escape_char(b: u8) -> char` / `pub fn unescape_char(c: char) ->
  Option<u8>` helpers.

## Ingress boundaries (bytes → encoded String)

1. Shell source input: `main.rs` stdin script / `--parse -` (`Read::read_to_end`
   + encode), `run_file` (`fs::read` + encode), `-c` operand and all argv via
   `std::env::args_os()` (encode each), `source`/`.` builtin
   (`src/exec/mod.rs:107`), `fc` temp-file re-read (`src/builtin/special.rs`).
2. `$'\xHH'`/`\NNN` escapes: `src/lexer/word.rs` `read_dollar_single_quote`
   final conversion `from_utf8_lossy` → `encode_bytes`.
3. Command substitution capture: `src/expand/command_sub.rs:93`
   `from_utf8_lossy` → `encode_bytes`.
4. `read` builtin field decode: `src/builtin/read.rs::field_to_string`
   `from_utf8_lossy` → `encode_bytes`.
5. Environment import: `VarStore::from_environ` switches from
   `std::env::vars()` (drops non-UTF-8 pairs) to `std::env::vars_os()` +
   encode of name and value.
6. Pathname expansion: `src/expand/pathname.rs::glob_in_dir` stops skipping
   non-UTF-8 entries; encodes `OsStr` bytes (`OsStrExt::as_bytes`) so they
   match patterns and produce fields.
7. Tilde expansion: `pw_dir` `to_string_lossy` → encode (`src/expand/tilde.rs`).
8. `cd`: `current_dir()` `to_string_lossy` → encode (`src/builtin/regular.rs`).
9. Interactive line input is normalized with `encode_bytes` before execution
   (no-op for ordinary UTF-8 input).

## Egress boundaries (encoded String → bytes)

1. External exec: `build_exec_cstrings_for_path` decodes argv;
   the child-side `setenv` loop decodes name/value; `exec_external_absolute`
   passes decoded bytes as `OsString` to `std::process::Command`.
2. Redirect targets: decode → `Path::new(OsStr::from_bytes(..))` for `open`.
3. `cd`: decode before `set_current_dir`; `test`/`[` file operators decode
   paths before metadata calls.
4. Word-producing builtins: `echo`, `printf` write decoded bytes to stdout.
   Listing builtins that print variable data (`export -p`, `set`, `env`
   snapshot, `alias`, `trap` output, `getopts` diagnostics) decode via a
   shared stdout-bytes helper.
5. Here-document bodies decode before being written to the pipe.
6. PATH search decodes candidate paths before `access`/`stat`.
7. Prompt rendering decodes before writing to the terminal.

## Plugin API byte-semantics decision

WIT `string` is UTF-8 by definition. Decision: the plugin surface stays
UTF-8. At the host boundary, values are decoded from the internal encoding
and then converted with `from_utf8_lossy`, so plugins observe U+FFFD for
raw invalid bytes and never observe escape codepoints. Documented here and
in TODO removal; no WIT change.

## Known, accepted divergences

- Collation: `test`/`[` supports only `=`/`!=` (POSIX), and the encoding is
  injective, so string equality is byte-exact — no divergence there (locked
  in by `string_equality_is_byte_exact_for_invalid_utf8`). Ordering of
  encoded forms surfaces only in pattern bracket ranges: escaped-byte
  endpoints are monotonic in the raw byte value (byte-order faithful; see
  `test_bracket_range_over_escaped_bytes_follows_byte_order`), while a
  *mixed* range between an invalid byte and a multi-byte UTF-8 char
  compares codepoints — a corner that is locale-dependent/undefined in
  other shells too.
- An invalid byte cannot serve as an IFS delimiter (non-whitespace IFS
  remains ASCII-restricted).
- History files store the encoded (valid UTF-8) form.
- Interactive prompt/syntax-highlight rendering displays escape codepoints
  as replacement glyphs rather than writing raw bytes to the terminal;
  script and pipeline output are unaffected (`echo`/`printf`/exec decode).

## Testing

- Unit: `byteenc` round-trip (all 256 single bytes, mixed sequences,
  real escape-range chars, empty), fast-path borrow assertions.
- Unit: `$'\xe9'` lexes to escape char; command-sub/`read` preserve bytes.
- E2E (`e2e/posix_spec/`): `$'\xe9'` through `printf`/pipe to `od`;
  `x=$(printf '\xe9')` round trip; `read` from a file with invalid bytes;
  glob matching a non-UTF-8 filename; exporting an invalid-byte value to an
  external command. Assertions go through `od -An -tx1`-style normalization
  so the test metadata stays ASCII.
- Full `cargo test` + `./e2e/run_tests.sh`.
