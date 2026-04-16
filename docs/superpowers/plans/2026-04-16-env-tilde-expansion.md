# ENV Tilde Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand `~` and `~user` prefixes in `$ENV` values before parameter expansion, following POSIX 2.6.1 ordering.

**Architecture:** Pre-process tilde prefix in `src/interactive/mod.rs` before the existing double-quote-wrap + parameter expansion. Reuse `expand_tilde_user` from `src/expand/mod.rs` by making it `pub(crate)`.

**Tech Stack:** Rust, libc (getpwnam)

---

### Task 1: Make `expand_tilde_user` pub(crate)

**Files:**
- Modify: `src/expand/mod.rs:529`

- [ ] **Step 1: Change visibility**

In `src/expand/mod.rs`, change line 529 from:

```rust
fn expand_tilde_user(user: &str) -> String {
```

to:

```rust
pub(crate) fn expand_tilde_user(user: &str) -> String {
```

- [ ] **Step 2: Verify existing tests still pass**

Run: `cargo test -p yosh expand_tilde`
Expected: `test_tilde_root_starts_with_slash` and related tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/expand/mod.rs
git commit -m "refactor: make expand_tilde_user pub(crate) for reuse in ENV expansion"
```

---

### Task 2: Add tilde expansion to ENV processing

**Files:**
- Modify: `src/interactive/mod.rs:67-90`

- [ ] **Step 1: Add tilde pre-processing before double-quote wrapping**

In `src/interactive/mod.rs`, replace the ENV processing block (lines 67-90):

```rust
        // Source $ENV (POSIX: parameter-expanded path for interactive shells)
        if let Some(env_val) = executor.env.vars.get("ENV").map(|s| s.to_string()) {
            if !env_val.is_empty() {
                // Parse as double-quoted word for parameter expansion
                let input = format!("\"{}\"", env_val);
                let expanded = match crate::lexer::Lexer::new(&input).next_token() {
                    Ok(tok) => {
                        if let crate::lexer::token::Token::Word(word) = tok.token {
                            crate::expand::expand_word_to_string(&mut executor.env, &word)
                                .ok()
                                .or_else(|| Some(env_val.clone()))
                        } else {
                            Some(env_val.clone())
                        }
                    }
                    Err(_) => Some(env_val.clone()),
                };
                if let Some(path) = expanded {
                    if executor.source_file(std::path::Path::new(&path)).is_none() {
                        eprintln!("yosh: {}: No such file or directory", path);
                    }
                }
            }
        }
```

with:

```rust
        // Source $ENV (POSIX: parameter-expanded path for interactive shells)
        if let Some(env_val) = executor.env.vars.get("ENV").map(|s| s.to_string()) {
            if !env_val.is_empty() {
                // POSIX 2.6.1: tilde expansion occurs before parameter expansion
                let after_tilde = if let Some(rest) = env_val.strip_prefix('~') {
                    let (user, suffix) = match rest.find('/') {
                        Some(pos) => (&rest[..pos], &rest[pos..]),
                        None => (rest, ""),
                    };
                    if user.is_empty() {
                        // ~ alone: use $HOME from shell environment
                        match executor.env.vars.get("HOME").map(|s| s.to_string()) {
                            Some(home) if !home.is_empty() => format!("{}{}", home, suffix),
                            _ => env_val.clone(), // no HOME, keep original
                        }
                    } else {
                        // ~user: resolve via getpwnam
                        let expanded = crate::expand::expand_tilde_user(user);
                        if expanded.starts_with('~') {
                            env_val.clone() // unknown user, keep original
                        } else {
                            format!("{}{}", expanded, suffix)
                        }
                    }
                } else {
                    env_val.clone()
                };

                // Parse as double-quoted word for parameter expansion
                let input = format!("\"{}\"", after_tilde);
                let expanded = match crate::lexer::Lexer::new(&input).next_token() {
                    Ok(tok) => {
                        if let crate::lexer::token::Token::Word(word) = tok.token {
                            crate::expand::expand_word_to_string(&mut executor.env, &word)
                                .ok()
                                .or_else(|| Some(after_tilde.clone()))
                        } else {
                            Some(after_tilde.clone())
                        }
                    }
                    Err(_) => Some(after_tilde.clone()),
                };
                if let Some(path) = expanded {
                    if executor.source_file(std::path::Path::new(&path)).is_none() {
                        eprintln!("yosh: {}: No such file or directory", path);
                    }
                }
            }
        }
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build -p yosh 2>&1`
Expected: Compiles without errors or warnings

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p yosh`
Expected: All existing tests PASS (no regressions)

- [ ] **Step 4: Run E2E tests for ENV**

Run: `./e2e/run_tests.sh --filter=source_env`
Expected: `source_env.sh` and `source_env_expansion.sh` PASS

- [ ] **Step 5: Commit**

```bash
git add src/interactive/mod.rs
git commit -m "feat: add tilde expansion for ENV variable on interactive startup

POSIX 2.6.1 specifies tilde expansion before parameter expansion.
ENV=~/foo and ENV=~user/foo are now expanded correctly."
```

---

### Task 3: Update TODO.md

**Files:**
- Modify: `TODO.md:19`

- [ ] **Step 1: Delete the completed TODO item**

Remove line 19 from `TODO.md`:

```
- [ ] `ENV` tilde expansion — `ENV=~/foo` is not expanded because the value is parsed in double-quote context; POSIX only requires parameter expansion, but tilde support is practically expected (`src/interactive/mod.rs`)
```

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: remove completed ENV tilde expansion item from TODO.md"
```
