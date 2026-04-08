# Phase 4: Here-Document I/O Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement here-document I/O so `<<EOF` and `<<-EOF` feed their body content to command stdin, with proper expansion for unquoted delimiters.

**Architecture:** When a HereDoc redirect is encountered: expand the body (parameter/command/arithmetic expansion if delimiter was unquoted, literal if quoted), write it to a pipe, and connect the pipe's read end to the command's stdin via dup2. Uses the existing Phase 3 expansion pipeline.

**Tech Stack:** Rust 2024, `nix` 0.31, `libc` 0.2

**Scope note:** This is Phase 4 of 8. Basic file redirections (<, >, >>, >|, <&, >&, <>) were implemented in Phase 2 and remain unchanged. This phase only adds here-document support. noclobber (set -C) is deferred to Phase 6 (shell options).

---

## File Structure

**Modify:**
- `src/expand/mod.rs` — Add `expand_heredoc_body` public function
- `src/exec/redirect.rs` — Replace HereDoc stub with pipe-based implementation

**Test:**
- `tests/parser_integration.rs` — Add here-document integration tests

---

### Task 1: Here-document body expansion

**Files:**
- Modify: `src/expand/mod.rs`

- [ ] **Step 1: Write tests**

Add to `src/expand/mod.rs` tests module:

```rust
    #[test]
    fn test_expand_heredoc_body_literal() {
        let mut env = make_env();
        let parts = vec![WordPart::Literal("hello world\n".to_string())];
        assert_eq!(expand_heredoc_body(&mut env, &parts, true), "hello world\n");
    }

    #[test]
    fn test_expand_heredoc_body_quoted_no_expansion() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        // Quoted delimiter: body contains $FOO literally, should NOT expand
        let parts = vec![WordPart::Literal("value is $FOO\n".to_string())];
        assert_eq!(expand_heredoc_body(&mut env, &parts, true), "value is $FOO\n");
    }

    #[test]
    fn test_expand_heredoc_body_unquoted_expands() {
        let mut env = make_env();
        env.vars.set("FOO", "bar").unwrap();
        // Unquoted delimiter: body parts include Parameter expansion
        let parts = vec![
            WordPart::Literal("value is ".to_string()),
            WordPart::Parameter(ParamExpr::Simple("FOO".to_string())),
            WordPart::Literal("\n".to_string()),
        ];
        assert_eq!(expand_heredoc_body(&mut env, &parts, false), "value is bar\n");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test expand::tests::test_expand_heredoc`
Expected: FAIL (function not defined).

- [ ] **Step 3: Implement expand_heredoc_body**

Add to `src/expand/mod.rs` (public function):

```rust
/// Expand a here-document body.
/// If `quoted` is true (delimiter was quoted), body is literal — no expansion.
/// If `quoted` is false, parameter expansion, command substitution, and arithmetic
/// expansion are performed (same as double-quote context, but `"` is not special).
pub fn expand_heredoc_body(env: &mut ShellEnv, parts: &[WordPart], quoted: bool) -> String {
    if quoted {
        // Quoted delimiter: just concatenate all literal parts
        let mut result = String::new();
        for part in parts {
            if let WordPart::Literal(s) = part {
                result.push_str(s);
            }
        }
        result
    } else {
        // Unquoted delimiter: expand like double-quoted context
        let mut result = String::new();
        for part in parts {
            expand_heredoc_part(env, part, &mut result);
        }
        result
    }
}

/// Expand a single WordPart in here-document context (similar to double-quote).
fn expand_heredoc_part(env: &mut ShellEnv, part: &WordPart, out: &mut String) {
    match part {
        WordPart::Literal(s) => out.push_str(s),
        WordPart::Parameter(p) => {
            let expanded = param::expand(env, p);
            out.push_str(&expanded);
        }
        WordPart::CommandSub(program) => {
            let output = command_sub::execute(env, program);
            out.push_str(&output);
        }
        WordPart::ArithSub(expr) => {
            let result = arith::evaluate(env, expr);
            out.push_str(&result);
        }
        // In here-doc context, these should not appear but handle gracefully
        WordPart::SingleQuoted(s) => out.push_str(s),
        WordPart::DoubleQuoted(parts) => {
            for p in parts {
                expand_heredoc_part(env, p, out);
            }
        }
        WordPart::DollarSingleQuoted(s) => out.push_str(s),
        WordPart::Tilde(user) => {
            let expanded = expand_tilde(env, user.as_deref());
            out.push_str(&expanded);
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/expand/mod.rs
git commit -m "feat(phase4): add expand_heredoc_body for here-document expansion"
```

---

### Task 2: Here-document I/O via pipe

**Files:**
- Modify: `src/exec/redirect.rs`

- [ ] **Step 1: Write integration tests**

Add to `tests/parser_integration.rs`:

```rust
#[test]
fn test_heredoc_basic() {
    let out = kish_exec("cat <<EOF\nhello world\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_heredoc_multiline() {
    let out = kish_exec("cat <<EOF\nline1\nline2\nline3\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "line1\nline2\nline3\n");
}

#[test]
fn test_heredoc_with_variable_expansion() {
    let out = kish_exec("FOO=hello; cat <<EOF\nvalue is $FOO\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value is hello\n");
}

#[test]
fn test_heredoc_quoted_delimiter_no_expansion() {
    let out = kish_exec("FOO=hello; cat <<'EOF'\nvalue is $FOO\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value is $FOO\n");
}

#[test]
fn test_heredoc_strip_tabs() {
    let out = kish_exec("cat <<-EOF\n\thello\n\tworld\n\tEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nworld\n");
}

#[test]
fn test_heredoc_with_command() {
    let out = kish_exec("cat <<EOF | tr a-z A-Z\nhello\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "HELLO\n");
}

#[test]
fn test_heredoc_empty_body() {
    let out = kish_exec("cat <<EOF\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn test_heredoc_preserves_internal_newlines() {
    let out = kish_exec("cat <<EOF\n\n\nhello\n\n\nEOF");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "\n\nhello\n\n\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_heredoc_basic`
Expected: FAIL (heredoc body not fed to stdin).

- [ ] **Step 3: Implement HereDoc redirect**

In `src/exec/redirect.rs`, replace the HereDoc stub:

```rust
            RedirectKind::HereDoc(heredoc) => {
                let target_fd = redirect.fd.unwrap_or(0);

                // Expand the body
                let body = crate::expand::expand_heredoc_body(
                    env,
                    &heredoc.body,
                    heredoc.quoted,
                );

                // Create a pipe
                let mut fds: [RawFd; 2] = [0; 2];
                if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
                    return Err(format!("pipe: {}", std::io::Error::last_os_error()));
                }
                let (read_fd, write_fd) = (fds[0], fds[1]);

                // Write the body to the pipe
                use std::io::Write;
                use std::os::unix::io::FromRawFd;
                let mut write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
                let _ = write_file.write_all(body.as_bytes());
                drop(write_file); // closes write_fd

                // Connect read end to target fd
                if save {
                    self.save_fd(target_fd)?;
                }
                raw_dup2(read_fd, target_fd).map_err(|e| format!("dup2: {}", e))?;
                unsafe { libc::close(read_fd) };
            }
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/redirect.rs tests/parser_integration.rs
git commit -m "feat(phase4): here-document I/O via pipe with expansion support"
```

---

## Subsequent Phases

- **Phase 5:** Control structure execution (if, for, while, until, case, functions)
- **Phase 6:** Special builtins (set, export, trap, eval, exec) + alias expansion
- **Phase 7:** Signals and errexit
- **Phase 8:** Subshell environment isolation
