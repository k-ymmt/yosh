# POSIX Byte Semantics Stage 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish byte-oriented expansion-field boundaries while preserving current UTF-8 behavior.

**Architecture:** Keep parser, AST, variables, argv, and environment values as UTF-8 `String` for this stage. Add byte-facing APIs to `ExpandedField`, migrate expansion consumers to those APIs where byte offsets matter, centralize external-exec `CString` construction, and split the broad TODO into completed stage-1 work plus remaining full byte-transparency work.

**Tech Stack:** Rust, Cargo unit tests, POSIX shell expansion pipeline, `CString`, existing `ExpandedField` packed bit masks.

---

## File Structure

- Modify `src/expand/mod.rs`: add `ExpandedField` byte access/conversion helpers and tests for byte mask behavior.
- Modify `src/expand/field_split.rs`: use `ExpandedField` byte helpers in split logic and add multi-byte mask preservation tests.
- Modify `src/expand/pathname.rs`: use `ExpandedField` byte helpers in glob-metachar detection and add multi-byte glob-protection tests.
- Modify `src/exec/simple.rs`: add an exec-boundary `CString` helper and use it for command/argv construction.
- Modify `TODO.md`: replace the broad POSIX Byte Semantics entry with stage-1 completion notes and explicit remaining items.

## Task 1: Add Byte-Oriented ExpandedField API

**Files:**
- Modify: `src/expand/mod.rs`

- [ ] **Step 1: Write failing tests for byte access and byte-index masks**

Add these tests near the existing `ExpandedField` tests in `src/expand/mod.rs`:

```rust
#[test]
fn expanded_field_byte_access_reports_utf8_bytes() {
    let mut f = ExpandedField::new();
    f.push_literal("日*");

    assert_eq!(f.byte_len(), "日*".len());
    assert_eq!(f.as_bytes(), "日*".as_bytes());
    assert_eq!(f.into_string(), "日*");
}

#[test]
fn expanded_field_masks_are_indexed_by_byte() {
    let mut f = ExpandedField::new();
    f.push_quoted("日");
    f.push_literal("*");
    f.push_expanded(" 本");

    assert_eq!(f.byte_len(), "日* 本".len());

    for i in 0.."日".len() {
        assert!(f.is_split_protected(i), "byte {i} split-protected");
        assert!(f.is_glob_protected(i), "byte {i} glob-protected");
    }

    let star = "日".len();
    assert!(f.is_split_protected(star));
    assert!(!f.is_glob_protected(star));

    let expanded_start = star + 1;
    for i in expanded_start..f.byte_len() {
        assert!(!f.is_split_protected(i), "byte {i} split-subject");
        assert!(!f.is_glob_protected(i), "byte {i} glob-subject");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test expanded_field_
```

Expected: compilation fails because `ExpandedField::byte_len`, `ExpandedField::as_bytes`, and `ExpandedField::into_string` do not exist.

- [ ] **Step 3: Add minimal byte helper implementation**

Add these methods to `impl ExpandedField` in `src/expand/mod.rs`, after `is_empty`:

```rust
    /// Return the field value as bytes.
    ///
    /// Stage 1 still stores valid UTF-8 in `String`; callers that reason about
    /// split/glob masks should go through this byte-facing API so later raw
    /// byte storage has one obvious migration point.
    pub fn as_bytes(&self) -> &[u8] {
        self.value.as_bytes()
    }

    /// Return the field length in bytes, matching mask indexing.
    pub fn byte_len(&self) -> usize {
        self.value.len()
    }

    /// Consume the field and return its current UTF-8 value.
    pub fn into_string(self) -> String {
        self.value
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test expanded_field_
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/expand/mod.rs
git commit -m "feat(expand): expose byte helpers on expanded fields"
```

## Task 2: Use Byte Helpers In Field Splitting

**Files:**
- Modify: `src/expand/field_split.rs`

- [ ] **Step 1: Write failing/pinning tests for multi-byte mask preservation**

Add these tests at the end of the `#[cfg(test)] mod tests` in `src/expand/field_split.rs`:

```rust
#[test]
fn split_preserves_quoted_multibyte_masks() {
    let env = env_with_ifs(":");
    let mut f = ExpandedField::new();
    f.push_quoted("日:本");

    let result = split(&env, vec![f]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].value, "日:本");
    for i in 0..result[0].byte_len() {
        assert!(result[0].is_split_protected(i), "byte {i} split-protected");
        assert!(result[0].is_glob_protected(i), "byte {i} glob-protected");
    }
}

#[test]
fn split_preserves_literal_multibyte_as_split_protected_glob_subject() {
    let env = env_with_ifs(":");
    let mut f = ExpandedField::new();
    f.push_literal("日:本*");

    let result = split(&env, vec![f]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].value, "日:本*");
    for i in 0..result[0].byte_len() {
        assert!(result[0].is_split_protected(i), "byte {i} split-protected");
    }
    let star = "日:本".len();
    assert!(!result[0].is_glob_protected(star));
}

#[test]
fn split_expanded_multibyte_remains_split_subject() {
    let env = env_with_ifs(":");
    let mut f = ExpandedField::new();
    f.push_expanded("日:本");

    let result = split(&env, vec![f]);
    assert_eq!(values(result), vec!["日", "本"]);
}
```

- [ ] **Step 2: Run tests to verify current behavior**

Run:

```bash
cargo test split_
```

Expected: tests may already pass because the current implementation is byte-mask based. Passing is acceptable here; the tests are regression pins required by the spec.

- [ ] **Step 3: Replace direct value byte access in splitter**

In `src/expand/field_split.rs`, make these mechanical changes:

```rust
// before
.filter(|f| !f.value.is_empty() || f.was_quoted)

// after
.filter(|f| f.byte_len() != 0 || f.was_quoted)
```

```rust
// before
let bytes = field.value.as_bytes();
let len = bytes.len();

// after
let bytes = field.as_bytes();
let len = field.byte_len();
```

```rust
// before
field
    .value
    .bytes()
    .enumerate()

// after
field
    .as_bytes()
    .iter()
    .copied()
    .enumerate()
```

Leave `append_char` string slicing unchanged in this task because it needs UTF-8 character slices while stage 1 still stores `String`.

- [ ] **Step 4: Run focused field split tests**

Run:

```bash
cargo test field_split::
```

Expected: all `field_split` tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/expand/field_split.rs
git commit -m "test(expand): pin field splitting byte masks"
```

## Task 3: Use Byte Helpers In Pathname Expansion

**Files:**
- Modify: `src/expand/pathname.rs`

- [ ] **Step 1: Write tests for multi-byte glob protection**

Add these tests at the end of the `#[cfg(test)] mod tests` in `src/expand/pathname.rs`:

```rust
#[test]
fn multibyte_literal_star_remains_glob_subject() {
    let mut f = ExpandedField::new();
    f.push_literal("日*");

    assert_eq!(f.byte_len(), "日*".len());
    assert!(has_unquoted_glob_chars(&f));
}

#[test]
fn multibyte_quoted_star_is_glob_protected() {
    let mut f = ExpandedField::new();
    f.push_quoted("日*");

    assert_eq!(f.byte_len(), "日*".len());
    assert!(!has_unquoted_glob_chars(&f));
}

#[test]
fn multibyte_expanded_question_remains_glob_subject() {
    let mut f = ExpandedField::new();
    f.push_expanded("本?");

    assert_eq!(f.byte_len(), "本?".len());
    assert!(has_unquoted_glob_chars(&f));
}
```

- [ ] **Step 2: Run tests to verify current behavior**

Run:

```bash
cargo test multibyte_
```

Expected: tests may already pass. Passing is acceptable because the task pins the byte-level behavior.

- [ ] **Step 3: Replace direct value byte access in pathname helpers**

In `src/expand/pathname.rs`, update `has_unquoted_glob_chars`:

```rust
fn has_unquoted_glob_chars(field: &ExpandedField) -> bool {
    for (i, &b) in field.as_bytes().iter().enumerate() {
        if !field.is_glob_protected(i) && matches!(b, b'*' | b'?' | b'[') {
            return true;
        }
    }
    false
}
```

Do not change `glob_match(&field.value)` yet; matching still accepts `&str` in stage 1.

- [ ] **Step 4: Run focused pathname tests**

Run:

```bash
cargo test pathname::
```

Expected: all `pathname` tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/expand/pathname.rs
git commit -m "test(expand): pin pathname byte glob masks"
```

## Task 4: Centralize Exec CString Boundary

**Files:**
- Modify: `src/exec/simple.rs`

- [ ] **Step 1: Write unit tests for command and argv CString conversion**

Add this helper test module content near the existing tests in `src/exec/simple.rs`:

```rust
#[test]
fn build_exec_cstrings_accepts_valid_utf8_args() {
    let args = vec!["arg".to_string(), "日本".to_string()];
    let (cmd, argv) = build_exec_cstrings("echo", &args).expect("valid args");

    assert_eq!(cmd.as_bytes(), b"echo");
    assert_eq!(argv.len(), 3);
    assert_eq!(argv[0].as_bytes(), b"echo");
    assert_eq!(argv[1].as_bytes(), b"arg");
    assert_eq!(argv[2].as_bytes(), "日本".as_bytes());
}

#[test]
fn build_exec_cstrings_rejects_nul_in_command_name() {
    let args = Vec::new();
    let err = build_exec_cstrings("bad\0cmd", &args).unwrap_err();
    assert_eq!(err, ExecCStringError::CommandName);
}

#[test]
fn build_exec_cstrings_rejects_nul_in_argument() {
    let args = vec!["bad\0arg".to_string()];
    let err = build_exec_cstrings("echo", &args).unwrap_err();
    assert_eq!(err, ExecCStringError::Argument("bad\0arg".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test build_exec_cstrings
```

Expected: compilation fails because `build_exec_cstrings` and `ExecCStringError` do not exist.

- [ ] **Step 3: Add exec CString helper**

Add this code above `impl Executor` in `src/exec/simple.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecCStringError {
    CommandName,
    Argument(String),
}

fn build_exec_cstrings(
    cmd: &str,
    args: &[String],
) -> Result<(CString, Vec<CString>), ExecCStringError> {
    let c_cmd = CString::new(cmd).map_err(|_| ExecCStringError::CommandName)?;
    let mut c_args = Vec::with_capacity(args.len() + 1);
    c_args.push(c_cmd.clone());
    for arg in args {
        let c_arg = CString::new(arg.as_str())
            .map_err(|_| ExecCStringError::Argument(arg.clone()))?;
        c_args.push(c_arg);
    }
    Ok((c_cmd, c_args))
}
```

- [ ] **Step 4: Use helper in external exec**

Replace the current `CString::new` block at the start of `exec_external_with_redirects` with:

```rust
        let (c_cmd, c_args) = match build_exec_cstrings(cmd, args) {
            Ok(v) => v,
            Err(ExecCStringError::CommandName) => {
                eprintln!("yosh: {}: invalid command name", cmd);
                return 127;
            }
            Err(ExecCStringError::Argument(arg)) => {
                eprintln!("yosh: {}: invalid argument", arg);
                return 1;
            }
        };
```

- [ ] **Step 5: Run focused exec tests**

Run:

```bash
cargo test build_exec_cstrings
```

Expected: all three tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/exec/simple.rs
git commit -m "refactor(exec): centralize cstring argv boundary"
```

## Task 5: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Update POSIX Byte Semantics entry**

Replace the current `## Future: POSIX Byte Semantics` bullet with:

```markdown
## Future: POSIX Byte Semantics

- [ ] Complete full non-UTF-8 shell input, argv, paths, and environment value
      support. Stage 1 established byte-oriented expansion-field APIs and
      regression tests around byte-index split/glob protection, plus a
      centralized UTF-8 `CString` boundary for external exec. Remaining work:
      migrate shell source input away from `read_to_string`; move AST word
      storage, variables, positional parameters, aliases, traps, and functions
      toward byte buffers plus quote/protection metadata; carry `OsString` or
      raw bytes through paths and process boundaries; and decide plugin API byte
      semantics. Keep this open until invalid UTF-8 data is preserved end to end.
```

- [ ] **Step 2: Run TODO sanity check**

Run:

```bash
sed -n '1,20p' TODO.md
```

Expected: the top POSIX Byte Semantics entry describes stage-1 completion and lists remaining work. It remains unchecked.

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs(todo): decompose POSIX byte semantics work"
```

## Task 6: Final Verification

**Files:**
- Verify: `src/expand/mod.rs`
- Verify: `src/expand/field_split.rs`
- Verify: `src/expand/pathname.rs`
- Verify: `src/exec/simple.rs`
- Verify: `TODO.md`

- [ ] **Step 1: Run focused test suites**

Run:

```bash
cargo test expand::
cargo test build_exec_cstrings
```

Expected: both commands pass.

- [ ] **Step 2: Run full test suite**

Run:

```bash
cargo test
```

Expected: full suite passes. If it fails due to pre-existing dirty worktree changes, record the failing test names and output summary before continuing.

- [ ] **Step 3: Run build**

Run:

```bash
cargo build
```

Expected: build passes.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short
git log --oneline -6
```

Expected: worktree still may contain unrelated pre-existing modifications, but task commits should include only the files listed in this plan.
