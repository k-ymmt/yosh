# SP3: Native `read` Builtin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement POSIX `read [-r] var ...` as a native yosh builtin so 9 SP3 XFAIL tests pass, dropping the E2E XFail count from 39 to 30.

**Architecture:** New `src/builtin/read.rs` module wired into `src/builtin/mod.rs` (4 sites: `pub mod`, `BUILTIN_NAMES`, `classify_builtin`, `exec_regular_builtin`). Internal split: pure `parse_args` → trait-injectable `read_logical_line` (1-byte stdin via `libc::read` in production, `Cursor` in tests) → `split_and_assign` (POSIX §2.6.5 field-splitting, last-var-gets-remainder, escape-aware). The executor's `exec`-no-command persistent-redirect path already works (`src/exec/simple.rs:316-329`); no executor changes.

**Tech Stack:** Rust 2024, `libc` (already a dep), `std::io::Cursor` for tests. No new dependencies.

**Reference:** [`docs/superpowers/specs/2026-05-14-e2e-xfail-sp3-read-builtin-design.md`](../specs/2026-05-14-e2e-xfail-sp3-read-builtin-design.md).

---

## File Map

| Path | Action | Responsibility |
|------|--------|---------------|
| `src/builtin/read.rs` | Create | `builtin_read` + 3 internal helpers + unit tests |
| `src/builtin/mod.rs` | Modify | Module declaration, BUILTIN_NAMES, classify_builtin, dispatch |
| `e2e/posix_spec/4_required_builtin/read_basic.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/read_partial_line.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/read_multiple_vars.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/read_no_args.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/read_last_var_gets_remainder.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/read_r_preserves_backslash.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_required_builtin/read_strips_ifs.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_special_builtin/exec_redir_input.sh` | Modify | Remove `# XFAIL:` line |
| `e2e/posix_spec/4_special_builtin/exec_close_fd.sh` | Modify | Verify against bash/dash; adjust `EXPECT_EXIT` if needed; remove XFAIL |
| `TODO.md` | Modify | Remove SP3 row + `read [-r] var...` row; optionally add SP3 follow-ups |
| `~/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md` | Modify | Mark SP3 COMPLETE |

---

## Task 1: Wire `read` into builtin classification (TDD)

**Files:**
- Modify: `src/builtin/mod.rs:1-7, 12-18, 33-41, 44-76, 87-130`
- Create: `src/builtin/read.rs` (stub returning `Ok(1)` with "not implemented" stderr)

- [ ] **Step 1: Create stub `src/builtin/read.rs`**

```rust
//! POSIX `read` builtin.
//!
//! `read [-r] var [var ...]` — read one logical line from stdin and
//! assign IFS-split fields to the named variables. With no `-r`,
//! backslash is the escape character (line continuation on `\<newline>`,
//! `\X` keeps `X` literally).

use crate::env::ShellEnv;
use crate::error::ShellError;

pub fn builtin_read(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    eprintln!("yosh: read: not implemented");
    Ok(1)
}
```

- [ ] **Step 2: Add module declaration to `src/builtin/mod.rs`**

Insert `pub mod read;` in the existing module-list block at the top of the file, keeping alphabetical-ish order (after `pub mod regular;`, before `pub mod resolve;`):

```rust
pub mod command;
pub mod hash;
pub mod read;          // ← new line
pub mod regular;
pub mod resolve;
pub mod special;
pub mod test;
pub mod r#type;
```

- [ ] **Step 3: Add `"read"` to `BUILTIN_NAMES` (line 12-18)**

Replace the regular-builtins half of the array (line 16-17) with:

```rust
    "cd", "command", "echo", "true", "false", "alias", "unalias", "kill", "wait", "fg", "bg",
    "jobs", "umask", "test", "[", "type", "hash", "read",
```

- [ ] **Step 4: Add `"read"` to `classify_builtin` (line 37-38)**

Replace the Regular arm so it reads:

```rust
        "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias" | "kill" | "wait"
        | "fg" | "bg" | "jobs" | "umask" | "test" | "[" | "type" | "hash" | "read" => BuiltinKind::Regular,
```

- [ ] **Step 5: Add `"read"` dispatch arm in `exec_regular_builtin` (line 44-76)**

Insert before the `_ => { ... }` fallback, right after the `"hash"` line (~line 71):

```rust
        "read" => read::builtin_read(args, env),
```

- [ ] **Step 6: Add a classify_builtin test in `src/builtin/mod.rs` tests module**

Locate the existing tests module (~line 86) and add this single line inside the existing `classifies_*` test function, next to the `"hash"` assertion (~line 118):

```rust
        assert!(matches!(classify_builtin("read"), BuiltinKind::Regular));
```

- [ ] **Step 7: Build and run the unit test**

Run:
```bash
cargo test -p yosh --lib builtin:: -- --nocapture 2>&1 | tail -20
```

Expected: tests pass, including the new `classify_builtin("read")` assertion. Build is green.

- [ ] **Step 8: Commit**

```bash
git add src/builtin/read.rs src/builtin/mod.rs
git commit -m "$(cat <<'EOF'
feat(builtin): add read module skeleton + classify wiring

Stub builtin_read() returning exit 1 "not implemented" so the four
mod.rs wiring sites (pub mod, BUILTIN_NAMES, classify_builtin,
exec_regular_builtin dispatch) can land first under TDD.
Subsequent tasks implement parse_args, read_logical_line, and
split_and_assign.

SP3 of E2E XFAIL roadmap. Spec:
docs/superpowers/specs/2026-05-14-e2e-xfail-sp3-read-builtin-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `ParsedArgs` + `parse_args` helper (TDD)

**Files:**
- Modify: `src/builtin/read.rs` (add `ParsedArgs`, `parse_args`, 5 unit tests)

- [ ] **Step 1: Append `ParsedArgs` struct, `ArgError` enum, and unit tests to `src/builtin/read.rs`**

Append below the existing stub `builtin_read`:

```rust
#[derive(Debug, PartialEq)]
struct ParsedArgs {
    raw: bool,
    var_names: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum ArgError {
    NoVarName,
    UnknownFlag(char),
    InvalidIdentifier(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_args_no_args_is_error() {
        assert_eq!(parse_args(&[]), Err(ArgError::NoVarName));
    }

    #[test]
    fn parse_args_single_var() {
        assert_eq!(
            parse_args(&s(&["line"])),
            Ok(ParsedArgs { raw: false, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_dash_r_sets_raw() {
        assert_eq!(
            parse_args(&s(&["-r", "line"])),
            Ok(ParsedArgs { raw: true, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_double_dash_terminates_options() {
        // After `--`, even `-r` is treated as a (invalid) variable name.
        assert_eq!(
            parse_args(&s(&["--", "line"])),
            Ok(ParsedArgs { raw: false, var_names: vec!["line".into()] })
        );
    }

    #[test]
    fn parse_args_double_dash_then_dash_r_treats_as_invalid_ident() {
        // `--` terminates options; subsequent `-r` is a name and fails validation.
        assert_eq!(
            parse_args(&s(&["--", "-r"])),
            Err(ArgError::InvalidIdentifier("-r".into()))
        );
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        assert_eq!(parse_args(&s(&["-x", "line"])), Err(ArgError::UnknownFlag('x')));
    }

    #[test]
    fn parse_args_invalid_identifier_errors() {
        assert_eq!(
            parse_args(&s(&["1foo"])),
            Err(ArgError::InvalidIdentifier("1foo".into()))
        );
    }

    #[test]
    fn parse_args_multiple_vars() {
        assert_eq!(
            parse_args(&s(&["-r", "x", "y", "z"])),
            Ok(ParsedArgs {
                raw: true,
                var_names: vec!["x".into(), "y".into(), "z".into()],
            })
        );
    }
}
```

- [ ] **Step 2: Run tests and confirm they fail (parse_args undefined)**

Run:
```bash
cargo test -p yosh --lib builtin::read:: 2>&1 | tail -15
```

Expected: compilation error `cannot find function 'parse_args'` (or similar).

- [ ] **Step 3: Implement `parse_args`**

Append to `src/builtin/read.rs` (above the `#[cfg(test)]` module):

```rust
fn parse_args(args: &[String]) -> Result<ParsedArgs, ArgError> {
    use crate::parser::word::is_valid_name;

    let mut raw = false;
    let mut idx = 0;
    while idx < args.len() {
        let a = &args[idx];
        if a == "--" {
            idx += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        for ch in a[1..].chars() {
            match ch {
                'r' => raw = true,
                other => return Err(ArgError::UnknownFlag(other)),
            }
        }
        idx += 1;
    }

    let var_names: Vec<String> = args[idx..].to_vec();
    if var_names.is_empty() {
        return Err(ArgError::NoVarName);
    }
    for name in &var_names {
        if !is_valid_name(name) {
            return Err(ArgError::InvalidIdentifier(name.clone()));
        }
    }
    Ok(ParsedArgs { raw, var_names })
}
```

- [ ] **Step 4: Run tests and confirm they pass**

Run:
```bash
cargo test -p yosh --lib builtin::read:: 2>&1 | tail -15
```

Expected: all 8 parse_args tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/read.rs
git commit -m "$(cat <<'EOF'
feat(builtin): read: parse_args helper with -r and -- support

POSIX read accepts only `-r` plus `--` option terminator. parse_args
returns ParsedArgs { raw, var_names } and reuses
parser::word::is_valid_name for identifier validation (same path as
export/unset/readonly).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `ByteReader` trait + `read_logical_line` (TDD)

**Files:**
- Modify: `src/builtin/read.rs`

- [ ] **Step 1: Append `ByteReader` trait, `LineByte` struct, `LineReadResult`, and unit tests**

Insert above the `#[cfg(test)]` module (or extend existing types section):

```rust
/// A single byte of the logical line, plus a flag for whether it came
/// through a backslash escape (so split_and_assign can ignore IFS
/// classification for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineByte {
    value: u8,
    escaped: bool,
}

#[derive(Debug, PartialEq)]
struct LineReadResult {
    bytes: Vec<LineByte>,
    /// `true` if input ended before a newline was seen (partial line or
    /// no input at all). The caller assigns the available bytes and
    /// returns exit 1.
    hit_eof: bool,
}

/// Byte-by-byte stdin reader, abstracted so unit tests can inject
/// in-memory input without touching fd 0.
trait ByteReader {
    /// Returns `Ok(Some(b))` for a byte, `Ok(None)` for EOF, or
    /// `Err(io::Error)` for genuine read failures. Implementors must
    /// retry on `EINTR` internally.
    fn read_byte(&mut self) -> std::io::Result<Option<u8>>;
}

#[cfg(test)]
struct SliceReader<'a> {
    src: &'a [u8],
    pos: usize,
}

#[cfg(test)]
impl<'a> SliceReader<'a> {
    fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }
}

#[cfg(test)]
impl<'a> ByteReader for SliceReader<'a> {
    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        if self.pos >= self.src.len() {
            Ok(None)
        } else {
            let b = self.src[self.pos];
            self.pos += 1;
            Ok(Some(b))
        }
    }
}
```

And append these tests inside the existing `mod tests` block (after the parse_args tests):

```rust
    fn lb(value: u8, escaped: bool) -> LineByte {
        LineByte { value, escaped }
    }

    #[test]
    fn read_line_basic_terminates_at_newline() {
        let mut r = SliceReader::new(b"hello\nworld\n");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res,
            LineReadResult {
                bytes: vec![lb(b'h', false), lb(b'e', false), lb(b'l', false), lb(b'l', false), lb(b'o', false)],
                hit_eof: false,
            }
        );
    }

    #[test]
    fn read_line_partial_line_signals_eof() {
        let mut r = SliceReader::new(b"partial");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res.bytes.iter().map(|b| b.value).collect::<Vec<_>>(),
            b"partial".to_vec()
        );
        assert!(res.hit_eof);
    }

    #[test]
    fn read_line_eof_with_no_bytes() {
        let mut r = SliceReader::new(b"");
        let res = read_logical_line(false, &mut r).unwrap();
        assert!(res.bytes.is_empty());
        assert!(res.hit_eof);
    }

    #[test]
    fn read_line_backslash_newline_continues() {
        let mut r = SliceReader::new(b"a\\\nb\n");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'b', false)],
        );
        assert!(!res.hit_eof);
    }

    #[test]
    fn read_line_backslash_other_keeps_literal_as_escaped() {
        let mut r = SliceReader::new(b"a\\bc\n");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'b', true), lb(b'c', false)],
        );
    }

    #[test]
    fn read_line_r_preserves_backslash_as_literal_byte() {
        let mut r = SliceReader::new(b"a\\b\n");
        let res = read_logical_line(true, &mut r).unwrap();
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'\\', false), lb(b'b', false)],
        );
    }

    #[test]
    fn read_line_r_backslash_newline_is_terminator() {
        // In -r mode, `\<newline>` is not line-continuation: the `\` is a
        // literal byte and the newline still ends the logical line.
        let mut r = SliceReader::new(b"a\\\nrest\n");
        let res = read_logical_line(true, &mut r).unwrap();
        assert_eq!(
            res.bytes,
            vec![lb(b'a', false), lb(b'\\', false)],
        );
        assert!(!res.hit_eof);
    }

    #[test]
    fn read_line_trailing_backslash_at_eof_in_nonraw_mode() {
        // Non-raw mode, input ends mid-escape ("a\" then EOF). Treat the
        // dangling backslash as if it were just dropped; bytes contains
        // only `a`, hit_eof=true.
        let mut r = SliceReader::new(b"a\\");
        let res = read_logical_line(false, &mut r).unwrap();
        assert_eq!(res.bytes, vec![lb(b'a', false)]);
        assert!(res.hit_eof);
    }
```

- [ ] **Step 2: Run tests, confirm failure**

Run:
```bash
cargo test -p yosh --lib builtin::read::tests 2>&1 | tail -20
```

Expected: compile error `cannot find function 'read_logical_line'`.

- [ ] **Step 3: Implement `read_logical_line`**

Insert above the test module:

```rust
fn read_logical_line<R: ByteReader>(raw: bool, reader: &mut R) -> std::io::Result<LineReadResult> {
    let mut bytes: Vec<LineByte> = Vec::new();
    loop {
        match reader.read_byte()? {
            None => return Ok(LineReadResult { bytes, hit_eof: true }),
            Some(b'\n') => return Ok(LineReadResult { bytes, hit_eof: false }),
            Some(b'\\') if !raw => {
                // Enter escape state.
                match reader.read_byte()? {
                    None => return Ok(LineReadResult { bytes, hit_eof: true }),
                    Some(b'\n') => continue, // line continuation: drop both
                    Some(other) => bytes.push(LineByte { value: other, escaped: true }),
                }
            }
            Some(other) => bytes.push(LineByte { value: other, escaped: false }),
        }
    }
}
```

- [ ] **Step 4: Run tests, confirm pass**

Run:
```bash
cargo test -p yosh --lib builtin::read::tests 2>&1 | tail -20
```

Expected: all 8 `read_line_*` tests PASS in addition to the parse_args tests.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/read.rs
git commit -m "$(cat <<'EOF'
feat(builtin): read: byte-by-byte logical-line reader

ByteReader trait abstracts fd 0 so tests inject SliceReader. Production
impl lands in a later task. read_logical_line implements the POSIX
backslash state machine: `\<newline>` line-continuation, `\X` records X
with escaped=true (so split treats it as literal), `-r` disables the
escape state entirely.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `split_and_assign` field-splitter (TDD)

**Files:**
- Modify: `src/builtin/read.rs`

- [ ] **Step 1: Add tests for the field-splitter (pure function, no ShellEnv)**

The split helper takes IFS + line bytes + var count and returns
`Vec<String>` of the same length as var count. Assignment to env
happens in a separate thin wrapper. Append inside `mod tests`:

```rust
    fn split_for(ifs: &str, line: Vec<LineByte>, n_vars: usize) -> Vec<String> {
        split_fields(ifs, &line, n_vars)
    }

    fn to_line(s: &str) -> Vec<LineByte> {
        s.bytes().map(|b| lb(b, false)).collect()
    }

    #[test]
    fn split_n_eq_1_trims_both_sides() {
        let out = split_for(" \t\n", to_line("   hello   "), 1);
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn split_n_eq_1_empty_input_yields_empty_string() {
        let out = split_for(" \t\n", to_line(""), 1);
        assert_eq!(out, vec!["".to_string()]);
    }

    #[test]
    fn split_n_gt_1_first_fields_then_remainder() {
        let out = split_for(" \t\n", to_line("a b c"), 3);
        assert_eq!(out, vec!["a".into(), "b".into(), "c".into()]);
    }

    #[test]
    fn split_remainder_keeps_internal_ifs() {
        let out = split_for(" \t\n", to_line("a b c d"), 2);
        assert_eq!(out, vec!["a".into(), "b c d".into()]);
    }

    #[test]
    fn split_leading_ifs_is_stripped() {
        let out = split_for(" \t\n", to_line("   a b"), 2);
        assert_eq!(out, vec!["a".into(), "b".into()]);
    }

    #[test]
    fn split_trailing_ws_ifs_stripped_from_remainder() {
        let out = split_for(" \t\n", to_line("a b c   "), 2);
        assert_eq!(out, vec!["a".into(), "b c".into()]);
    }

    #[test]
    fn split_more_vars_than_fields_yields_empty_strings() {
        let out = split_for(" \t\n", to_line("a"), 3);
        assert_eq!(out, vec!["a".into(), "".into(), "".into()]);
    }

    #[test]
    fn split_empty_ifs_no_split() {
        let out = split_for("", to_line("a b c"), 2);
        // No splitting at all → entire line in var[0], var[1] empty.
        assert_eq!(out, vec!["a b c".into(), "".into()]);
    }

    #[test]
    fn split_sep_ifs_treated_as_single_separator() {
        // `IFS=:`, input "a::b" → x=a, y="" (one colon separator), z=b
        let out = split_for(":", to_line("a::b"), 3);
        assert_eq!(out, vec!["a".into(), "".into(), "b".into()]);
    }

    #[test]
    fn split_mixed_sep_and_ws_ifs() {
        // IFS=":\t ", input "a: b" → ":" consumes one separator, then
        // " " is also IFS and is consumed greedily as adjacent ws.
        let out = split_for(": \t", to_line("a: b"), 2);
        assert_eq!(out, vec!["a".into(), "b".into()]);
    }

    #[test]
    fn split_escaped_byte_not_treated_as_ifs() {
        // Non-raw "a\ b": split would normally see " " as IFS, but the
        // space is escaped (came from `\<space>`), so it stays in field 1.
        let line = vec![
            lb(b'a', false),
            lb(b' ', true), // escaped space
            lb(b'b', false),
        ];
        let out = split_fields(" \t\n", &line, 2);
        assert_eq!(out, vec!["a b".into(), "".into()]);
    }
```

- [ ] **Step 2: Run tests, confirm failure**

Run:
```bash
cargo test -p yosh --lib builtin::read::tests::split_ 2>&1 | tail -20
```

Expected: compile error `cannot find function 'split_fields'`.

- [ ] **Step 3: Implement `split_fields`**

Insert above the test module (after `read_logical_line`):

```rust
/// POSIX §2.6.5 field splitting for `read`. Returns exactly `n_vars`
/// strings: var[0..n-1] are split fields, var[n-1] is the trailing
/// remainder (with only trailing whitespace-IFS trimmed). When IFS is
/// empty, no splitting occurs.
fn split_fields(ifs: &str, line: &[LineByte], n_vars: usize) -> Vec<String> {
    assert!(n_vars >= 1);

    // Classify IFS bytes.
    let mut ws_ifs: Vec<u8> = Vec::new();
    let mut sep_ifs: Vec<u8> = Vec::new();
    for b in ifs.bytes() {
        if b == b' ' || b == b'\t' || b == b'\n' {
            ws_ifs.push(b);
        } else {
            sep_ifs.push(b);
        }
    }

    // Helper: is byte `b` an unescaped IFS byte of the given class?
    let is_ws = |lb: &LineByte| !lb.escaped && ws_ifs.contains(&lb.value);
    let is_sep = |lb: &LineByte| !lb.escaped && sep_ifs.contains(&lb.value);
    let is_any_ifs = |lb: &LineByte| is_ws(lb) || is_sep(lb);

    // Empty IFS → no splitting at all.
    if ws_ifs.is_empty() && sep_ifs.is_empty() {
        let whole: String = line.iter().map(|b| b.value as char).collect();
        let mut out = vec![whole];
        out.extend((1..n_vars).map(|_| String::new()));
        return out;
    }

    // Trim leading ws_ifs.
    let mut i = 0;
    while i < line.len() && is_ws(&line[i]) {
        i += 1;
    }

    // N=1 shortcut: trim trailing ws_ifs and return the whole remainder.
    if n_vars == 1 {
        let mut j = line.len();
        while j > i && is_ws(&line[j - 1]) {
            j -= 1;
        }
        let s: String = line[i..j].iter().map(|b| b.value as char).collect();
        return vec![s];
    }

    let mut result: Vec<String> = Vec::with_capacity(n_vars);

    // Emit fields 0..n_vars-2 (each terminated by IFS).
    for _ in 0..(n_vars - 1) {
        if i >= line.len() {
            result.push(String::new());
            continue;
        }
        // Field bytes: until the next IFS or end-of-line.
        let start = i;
        while i < line.len() && !is_any_ifs(&line[i]) {
            i += 1;
        }
        let field: String = line[start..i].iter().map(|b| b.value as char).collect();
        result.push(field);

        // Consume one terminator: either a single sep_ifs byte plus
        // any adjacent ws_ifs, or a run of ws_ifs.
        if i < line.len() {
            if is_sep(&line[i]) {
                i += 1;
                // Adjacent ws_ifs collapses with the sep terminator.
                while i < line.len() && is_ws(&line[i]) {
                    i += 1;
                }
            } else {
                // ws_ifs run.
                while i < line.len() && is_ws(&line[i]) {
                    i += 1;
                }
            }
        }
    }

    // Remainder for var[n_vars-1]: trim trailing ws_ifs only.
    let mut j = line.len();
    while j > i && is_ws(&line[j - 1]) {
        j -= 1;
    }
    let remainder: String = line[i..j].iter().map(|b| b.value as char).collect();
    result.push(remainder);

    debug_assert_eq!(result.len(), n_vars);
    result
}
```

- [ ] **Step 4: Run tests, confirm pass**

Run:
```bash
cargo test -p yosh --lib builtin::read::tests 2>&1 | tail -25
```

Expected: all split_* tests PASS, plus parse_args + read_line tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add src/builtin/read.rs
git commit -m "$(cat <<'EOF'
feat(builtin): read: split_fields per POSIX 2.6.5

Classify IFS into whitespace-IFS and separator-IFS (POSIX requires
distinct collapsing semantics). N=1 case strips both ends; N>=2 case
emits N-1 split fields and gives the remainder (with only trailing
ws-IFS trimmed) to the last variable. Empty IFS = no splitting.
Escaped bytes (those that came through `\X` in non-raw mode) are
never treated as IFS.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Production `StdinByteReader` + wire `builtin_read`

**Files:**
- Modify: `src/builtin/read.rs`

- [ ] **Step 1: Replace the stub `builtin_read` with the real implementation**

Locate the stub at the top of `src/builtin/read.rs`:

```rust
pub fn builtin_read(_args: &[String], _env: &mut ShellEnv) -> Result<i32, ShellError> {
    eprintln!("yosh: read: not implemented");
    Ok(1)
}
```

Replace it with:

```rust
pub fn builtin_read(args: &[String], env: &mut ShellEnv) -> Result<i32, ShellError> {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(ArgError::NoVarName) => {
            eprintln!("yosh: read: missing variable name");
            return Ok(1);
        }
        Err(ArgError::UnknownFlag(c)) => {
            eprintln!("yosh: read: -{}: invalid option", c);
            return Ok(1);
        }
        Err(ArgError::InvalidIdentifier(name)) => {
            eprintln!("yosh: read: `{}': not a valid identifier", name);
            return Ok(1);
        }
    };

    let mut reader = StdinByteReader;
    let result = match read_logical_line(parsed.raw, &mut reader) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("yosh: read: {}", e);
            return Ok(1);
        }
    };

    let ifs = match env.vars.get("IFS") {
        Some(s) => s.to_string(),
        None => " \t\n".to_string(),
    };
    let values = split_fields(&ifs, &result.bytes, parsed.var_names.len());

    for (name, value) in parsed.var_names.iter().zip(values.into_iter()) {
        if let Err(e) = env.assign_var(name, value) {
            eprintln!("yosh: read: {}", e);
            return Ok(1);
        }
    }

    if result.hit_eof {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Production `ByteReader` reading 1 byte at a time from fd 0,
/// retrying on EINTR.
struct StdinByteReader;

impl ByteReader for StdinByteReader {
    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        let mut buf = [0u8; 1];
        loop {
            let n = unsafe {
                libc::read(libc::STDIN_FILENO, buf.as_mut_ptr() as *mut libc::c_void, 1)
            };
            if n == 1 {
                return Ok(Some(buf[0]));
            }
            if n == 0 {
                return Ok(None);
            }
            // n == -1: check errno
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }
    }
}
```

- [ ] **Step 2: Build the whole workspace**

Run:
```bash
cargo build -p yosh 2>&1 | tail -10
```

Expected: green build (no warnings on the new file).

- [ ] **Step 3: Manual smoke test (basic)**

Run:
```bash
./target/debug/yosh -c 'echo hello | { read line; echo "[$line]"; }'
```

Expected output: `[hello]`

- [ ] **Step 4: Manual smoke test (multi-var with remainder)**

Run:
```bash
./target/debug/yosh -c 'echo a b c d | { read x y; echo "x=[$x] y=[$y]"; }'
```

Expected output: `x=[a] y=[b c d]`

- [ ] **Step 5: Manual smoke test (partial line)**

Run:
```bash
./target/debug/yosh -c 'printf "partial" | { read line; rc=$?; echo "[$line] exit=$rc"; }'
```

Expected output: `[partial] exit=1`

(The intermediate `rc=$?` capture is essential — running `echo "exit=$?"`
directly would print the previous `echo "[$line]"`'s exit, not read's.)

- [ ] **Step 6: Manual smoke test (backslash escape, non-raw)**

Run:
```bash
./target/debug/yosh -c 'printf "a\\\\b\n" | { read line; echo "[$line]"; }'
```

Expected output: `[ab]` (the backslash is consumed; `b` stays literal).

- [ ] **Step 7: Manual smoke test (-r preserves backslash)**

Run:
```bash
./target/debug/yosh -c 'printf "a\\\\b\n" | { read -r line; echo "[$line]"; }'
```

Expected output: `[a\b]`

- [ ] **Step 8: Manual smoke test (custom IFS, separator splitting)**

Run:
```bash
./target/debug/yosh -c 'IFS=:; echo a::b | { read x y z; echo "x=[$x] y=[$y] z=[$z]"; }'
```

Expected output: `x=[a] y=[] z=[b]`

- [ ] **Step 9: Manual smoke test (no-args error)**

Run:
```bash
./target/debug/yosh -c 'read' 2>&1; echo "exit=$?"
```

Expected output:
```
yosh: read: missing variable name
exit=1
```

- [ ] **Step 10: Manual smoke test (`exec < file` integration)**

Run:
```bash
mkdir -p /tmp/yosh-sp3 && echo line1 > /tmp/yosh-sp3/in
./target/debug/yosh -c 'exec < /tmp/yosh-sp3/in; read line; echo "$line"'
```

Expected output: `line1`

- [ ] **Step 11: Commit**

```bash
git add src/builtin/read.rs
git commit -m "$(cat <<'EOF'
feat(builtin): read: wire up production stdin reader + entry point

builtin_read now: parses args, reads a logical line via libc::read
(1 byte at a time with EINTR retry), runs split_fields on the result
using IFS from env, assigns each value via env.assign_var (which
keeps the PATH-utility-hash cache coherent), and returns 1 on EOF /
partial line / argument error.

Unblocks SP3 E2E XFAIL removal in the next task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Strip XFAIL from 7 `read_*.sh` tests

**Files:**
- Modify: `e2e/posix_spec/4_required_builtin/read_basic.sh`
- Modify: `e2e/posix_spec/4_required_builtin/read_partial_line.sh`
- Modify: `e2e/posix_spec/4_required_builtin/read_multiple_vars.sh`
- Modify: `e2e/posix_spec/4_required_builtin/read_no_args.sh`
- Modify: `e2e/posix_spec/4_required_builtin/read_last_var_gets_remainder.sh`
- Modify: `e2e/posix_spec/4_required_builtin/read_r_preserves_backslash.sh`
- Modify: `e2e/posix_spec/4_required_builtin/read_strips_ifs.sh`

- [ ] **Step 1: Remove the `# XFAIL:` line from each of the 7 files**

For each file, delete the single line matching `# XFAIL: not yet implemented (TODO: implement read)`. Use the Edit tool per-file. After editing, the metadata header should still contain the `POSIX_REF`, `DESCRIPTION`, `EXPECT_*` lines but no `XFAIL` line.

Example for `read_basic.sh` — change from:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read assigns one line of stdin to a variable
# XFAIL: not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
echo hello | { read line; echo "$line"; }
```

To:

```sh
#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read assigns one line of stdin to a variable
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
echo hello | { read line; echo "$line"; }
```

Apply the same single-line deletion to the other 6 files.

- [ ] **Step 2: Run the 7 `read_*.sh` tests (excluding `read_eof_returns_nonzero.sh` which already passes)**

```bash
./e2e/run_tests.sh --filter=read_basic
./e2e/run_tests.sh --filter=read_partial_line
./e2e/run_tests.sh --filter=read_multiple_vars
./e2e/run_tests.sh --filter=read_no_args
./e2e/run_tests.sh --filter=read_last_var_gets_remainder
./e2e/run_tests.sh --filter=read_r_preserves_backslash
./e2e/run_tests.sh --filter=read_strips_ifs
```

Expected: each invocation shows `[PASS]` for the corresponding test and `Passed: 1  Failed: 0  …  XFail: 0`.

If any test fails: debug the implementation; do NOT re-add the XFAIL line. The runner does NOT auto-rebuild yosh, so re-run `cargo build` after any code change.

- [ ] **Step 3: Run the full `read_*` group together as a final check**

```bash
./e2e/run_tests.sh --filter=read_ 2>&1 | tail -15
```

Expected: 8 tests run (7 newly enabled + `read_eof_returns_nonzero`), all PASS, 0 XFail.

- [ ] **Step 4: Commit**

```bash
git add e2e/posix_spec/4_required_builtin/read_basic.sh \
        e2e/posix_spec/4_required_builtin/read_partial_line.sh \
        e2e/posix_spec/4_required_builtin/read_multiple_vars.sh \
        e2e/posix_spec/4_required_builtin/read_no_args.sh \
        e2e/posix_spec/4_required_builtin/read_last_var_gets_remainder.sh \
        e2e/posix_spec/4_required_builtin/read_r_preserves_backslash.sh \
        e2e/posix_spec/4_required_builtin/read_strips_ifs.sh
git commit -m "$(cat <<'EOF'
test(e2e): drop XFAIL from 7 read_* tests now that read is native

All 7 read-focused XFAIL tests pass against the new builtin:
- read_basic: line assignment
- read_partial_line: exit 1 on missing trailing newline
- read_multiple_vars: IFS-split into multiple vars
- read_no_args: error + exit 1 with stderr
- read_last_var_gets_remainder: trailing fields collapse into last var
- read_r_preserves_backslash: -r disables escape processing
- read_strips_ifs: leading/trailing whitespace-IFS stripped

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Strip XFAIL from `exec_redir_input.sh`

**Files:**
- Modify: `e2e/posix_spec/4_special_builtin/exec_redir_input.sh`

- [ ] **Step 1: Remove the `# XFAIL: ...` line**

Change from:

```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with no command applies input redirection to shell
# XFAIL: non-POSIX deviation (yosh exec < does not redirect shell stdin for subsequent read)
# EXPECT_OUTPUT: line1
# EXPECT_EXIT: 0
echo line1 > "$TEST_TMPDIR/in"
exec < "$TEST_TMPDIR/in"
read line
echo "$line"
```

To:

```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with no command applies input redirection to shell
# EXPECT_OUTPUT: line1
# EXPECT_EXIT: 0
echo line1 > "$TEST_TMPDIR/in"
exec < "$TEST_TMPDIR/in"
read line
echo "$line"
```

- [ ] **Step 2: Run the test**

```bash
./e2e/run_tests.sh --filter=exec_redir_input
```

Expected: `[PASS]  posix_spec/4_special_builtin/exec_redir_input.sh` with summary `Passed: 1  Failed: 0  XFail: 0`.

If FAIL: the test would mean either `exec <` no-command path or `read` regressed. The executor path at `src/exec/simple.rs:316-329` is the suspect; debug there.

- [ ] **Step 3: Also remove the matching TODO.md item under "Future: POSIX Conformance Bugs"**

In `TODO.md`, locate and delete the entire 3-line bullet:

```
- [ ] `exec <FILE` does not redirect the shell's stdin for subsequent
      commands (e.g., a following `read` does not see the file
      contents). XFAIL test:
      `e2e/posix_spec/4_special_builtin/exec_redir_input.sh`.
```

(This item is now resolved by the SP3 work; per the project convention, delete completed items rather than checking them off.)

- [ ] **Step 4: Commit**

```bash
git add e2e/posix_spec/4_special_builtin/exec_redir_input.sh TODO.md
git commit -m "$(cat <<'EOF'
test(e2e): drop XFAIL from exec_redir_input; close TODO item

`exec < FILE` (no command) persistent stdin redirect already worked
in the executor (src/exec/simple.rs:316-329); the test was XFAIL
only because the subsequent `read` could not see the redirected fd.
Native read closes the loop.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Verify and strip XFAIL from `exec_close_fd.sh`

**Files:**
- Modify: `e2e/posix_spec/4_special_builtin/exec_close_fd.sh`

**Background:** The test expects `EXPECT_EXIT: 0` for:
```sh
exec 3>&-
read line 0<&3 2>/dev/null
```
The spec §11.1 flagged this as likely wrong; this task verifies against bash/dash and adjusts if needed.

- [ ] **Step 1: Probe bash and dash to determine the POSIX-conforming exit**

Run:
```bash
bash -c 'exec 3>&-; read line 0<&3 2>/dev/null; echo $?'
dash -c 'exec 3>&-; read line 0<&3 2>/dev/null; echo $?'
```

Record both exit values. The conventional POSIX answer is `1` because `0<&3` against a closed fd is a redirection error in a non-special builtin.

- [ ] **Step 2: Choose action based on what bash/dash report**

| Bash | Dash | Action |
|------|------|--------|
| 1 | 1 | Set `EXPECT_EXIT: 1` in the test header (Step 3 below) |
| 0 | 0 | Keep `EXPECT_EXIT: 0`; verify yosh matches |
| differ | differ | Record divergence in TODO.md and pick the dash value (POSIX-leaning) |

- [ ] **Step 3: Update the test header to match the chosen exit + remove XFAIL**

Assuming bash and dash both report `1`, change the file from:

```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec N>&- closes fd N for the current shell
# XFAIL: not yet implemented (TODO: implement read; exec N>&- test verifies fd close via read)
# EXPECT_EXIT: 0
exec 3>&-
read line 0<&3 2>/dev/null
```

To:

```sh
#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec N>&- closes fd N for the current shell; reading from closed fd fails
# EXPECT_EXIT: 1
exec 3>&-
read line 0<&3 2>/dev/null
```

(If bash and dash both reported `0`, leave the EXPECT_EXIT line untouched and only remove the XFAIL line. Adjust DESCRIPTION wording accordingly.)

- [ ] **Step 4: Run the test**

```bash
./e2e/run_tests.sh --filter=exec_close_fd
```

Expected: `[PASS]  posix_spec/4_special_builtin/exec_close_fd.sh`.

If the test FAILs because yosh produces a different exit than bash/dash, investigate the redirection layer (`RedirectState::apply` handling of `<&CLOSED_FD`) — but DO NOT re-add the XFAIL line; either fix the executor or amend the test if it surfaces a real implementation choice.

- [ ] **Step 5: Commit**

Compose a commit message that includes the bash/dash probe result:

```bash
git add e2e/posix_spec/4_special_builtin/exec_close_fd.sh
git commit -m "$(cat <<'EOF'
test(e2e): exec_close_fd — match POSIX exit semantics + drop XFAIL

Probed bash/dash: both report exit 1 for `exec 3>&-; read line 0<&3
2>/dev/null` because dup-ing a closed fd is a redirection error in a
non-special builtin. Updated EXPECT_EXIT 0 → 1 to match. yosh now
PASSes the test natively.

(If your bash/dash probe in Step 1 reported 0, replace this paragraph
with the actual finding. Roadmap §5.3 permits fixing wrong stated
expectations in the same commit as the XFAIL removal.)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Full-suite verification + acceptance

- [ ] **Step 1: Run the full Rust test suite**

```bash
cargo test -p yosh --lib 2>&1 | tail -10
cargo test -p yosh --tests 2>&1 | tail -10
```

Expected: both green. No new failures.

- [ ] **Step 2: Run the full E2E suite**

```bash
./e2e/run_tests.sh 2>&1 | tail -10
```

Expected: summary line shows `XFail: 30` (down from 39). `Failed: 0`. No new `TIMEDOUT` from the SP3 area (the known flaky `PATH_search.sh` / `job_spec_prefix.sh` may still timeout intermittently per SP1 follow-ups — those are pre-existing).

If `XFail` is not exactly 30: count XFails per directory with:
```bash
grep -rln "# XFAIL:" /Users/kazukiyamamoto/Projects/rust/kish/e2e/posix_spec/ | wc -l
```
and reconcile (39 - 9 removed = 30).

- [ ] **Step 3: Capture the summary line for the next task's commit message**

Note the exact summary output (`Total: N  Passed: …  Failed: 0  Timedout: …  XFail: 30  XPass: 0`) for use in Task 10's commit body.

---

## Task 10: TODO.md + memory cleanup; SP3 closure commit

**Files:**
- Modify: `TODO.md`
- Modify: `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`

- [ ] **Step 1: Remove the SP3 line from the `## E2E XFAIL Roadmap` checklist**

In `TODO.md`, delete this single line under `## E2E XFAIL Roadmap`:

```
- [ ] SP3 — `read` builtin implementation (9 tests; includes `exec_close_fd` and `exec_redir_input`)
```

(Per project convention "Delete completed items from TODO.md rather than marking them with `[x]`".)

- [ ] **Step 2: Remove the `read [-r] var...` entry from "Future: POSIX Required Builtin Implementation"**

Delete this 4-line bullet:

```
- [ ] `read [-r] var...` — read one line from stdin into variables.
      Currently uses `/usr/bin/read`. XFAIL tests:
      `e2e/posix_spec/4_required_builtin/read_*.sh` (6 of 7 remain XFAIL —
      most cases require in-process state)
```

- [ ] **Step 3: Append SP3 follow-ups (if any surfaced during implementation) under a new heading**

If implementation surfaced any non-blocking cleanup items (e.g., a clippy nit, a shared helper to extract, a benchmark observation), add them under a new section after `### SP2 follow-ups (non-blocking)`:

```
### SP3 follow-ups (non-blocking)

- [ ] <each item, one paragraph, with file path + reason>
```

If nothing surfaced, skip this step — no empty section.

- [ ] **Step 4: Update the memory file**

Edit `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`:

- Change the `description:` frontmatter field to reflect SP1+SP2+SP3 complete and remaining XFail count of 30.
- In the body, update the status block: change `- **SP3 pending**: …` to `- **SP3 COMPLETE** (2026-05-14): 9 tests — native `read` builtin (POSIX `-r` only); `exec_close_fd.sh` and `exec_redir_input.sh` unblocked once read became native. Spec `2026-05-14-e2e-xfail-sp3-read-builtin-design.md`. <N> commits. Follow-ups under `### SP3 follow-ups (non-blocking)` in TODO.md (or "no follow-ups" if Step 3 was skipped).`
- Update `After SP1+SP2: 55 - 11 - 5 = 39 XFails remain` to `After SP1+SP2+SP3: 55 - 11 - 5 - 9 = 30 XFails remain`.
- If a Lessons-learned section is appropriate (e.g., interesting Rust patterns like `libc::read` EINTR loop or `ByteReader` trait injection for testing), add a short paragraph at the bottom.

- [ ] **Step 5: Update the top-level memory index**

Edit `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/MEMORY.md`:

Change the existing line:
```
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1+SP2 COMPLETE (2026-05-14, 39 XFails remain); SP3-SP7 pending
```

To:
```
- [E2E XFAIL roadmap status](project_e2e_xfail_roadmap.md) — SP1+SP2+SP3 COMPLETE (2026-05-14, 30 XFails remain); SP4-SP7 pending
```

- [ ] **Step 6: Commit (TODO.md + memory)**

```bash
git add TODO.md /Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md /Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/MEMORY.md
git commit -m "$(cat <<'EOF'
chore(sp3): close SP3 — remove roadmap entry and record follow-ups

E2E XFail count: 39 → 30 (paste actual summary line here).
9 tests removed XFAIL: read_basic, read_partial_line,
read_multiple_vars, read_no_args, read_last_var_gets_remainder,
read_r_preserves_backslash, read_strips_ifs, exec_close_fd,
exec_redir_input.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 7: Final sanity — fresh build + suite re-run from a clean state**

Run:
```bash
cargo build 2>&1 | tail -5
./e2e/run_tests.sh 2>&1 | tail -10
cargo test -p yosh --lib builtin::read 2>&1 | tail -10
```

Expected:
- `cargo build` green.
- E2E summary shows `XFail: 30  Failed: 0`.
- All unit tests in `builtin::read` PASS.

If anything regressed: investigate before declaring SP3 complete. Do not paper over with re-XFAILing.

---

## Acceptance Criterion (mirrors spec §12)

SP3 is complete when **all of the following** are true:

1. The 9 SP3 test files no longer carry `# XFAIL:` and every one reports PASS under `./e2e/run_tests.sh`.
2. `./e2e/run_tests.sh` end-of-suite summary shows `XFail: 30` (down from 39) with no new FAIL/TIMEDOUT.
3. `cargo test` (workspace) passes.
4. `cargo build` (workspace) passes.
5. `TODO.md` reflects closure: SP3 row removed from the roadmap checklist; `Future: POSIX Required Builtin Implementation` entry for `read [-r] var...` removed; any SP3 follow-up items recorded under `### SP3 follow-ups (non-blocking)`.
6. Memory: `project_e2e_xfail_roadmap.md` marks SP3 COMPLETE with date and follow-up status; `MEMORY.md` index line updated.
