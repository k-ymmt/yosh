# Phase 3: Full Word Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the complete POSIX word expansion pipeline — tilde, parameter, command substitution, arithmetic, field splitting, pathname expansion, and quote removal — so shell scripts with variables, globs, and command substitution work correctly.

**Architecture:** Refactor `src/expand/mod.rs` into a modular pipeline. Each Word produces `Vec<ExpandedField>` (value + per-character quoted_mask tracking which bytes are from expansion vs literal). The pipeline runs: expand parts → field split → pathname expand → quote remove → `Vec<String>`. `"$@"` in double quotes produces multiple fields. Pattern matching (shared module) is used by both parameter strip operations and pathname expansion.

**Tech Stack:** Rust 2024, `nix` 0.31 (for command substitution fork/pipe), `libc` 0.2

**Scope note:** This is Phase 3 of 8. Replaces the Phase 2 minimal expander with full POSIX-compliant expansion. Phase 4 (full redirection/here-doc I/O) and Phase 5 (control structures) follow.

---

## File Structure

**Create:**
- `src/expand/param.rs` — Parameter expansion (all conditional + strip forms)
- `src/expand/command_sub.rs` — Command substitution ($(...) and backtick)
- `src/expand/arith.rs` — Arithmetic expansion ($((expr)))
- `src/expand/field_split.rs` — IFS-based field splitting
- `src/expand/pathname.rs` — Pathname expansion (glob matching)
- `src/expand/pattern.rs` — Pattern matching (shared: param strip + pathname)

**Modify:**
- `src/expand/mod.rs` — Refactor: ExpandedField, pipeline orchestration, public API
- `src/exec/mod.rs` — Use new `expand_word`/`expand_words` API (takes `&mut ShellEnv`)
- `src/exec/redirect.rs` — Use `expand_word_to_string` with `&mut ShellEnv`
- `tests/parser_integration.rs` — Add expansion integration tests

**Reference:**
- `docs/posix-shell-reference.md` — Sections 2.6 (Expansions) and 2.14 (Pattern Matching)
- `src/parser/ast.rs` — Word, WordPart, ParamExpr, SpecialParam types

---

### Task 1: ExpandedField type and expansion pipeline refactor

**Files:**
- Modify: `src/expand/mod.rs`

Refactor the Phase 2 expander into a proper expansion pipeline. The key change: expansion produces `ExpandedField` (value + per-character quoting info) to support field splitting and pathname expansion decisions.

- [ ] **Step 1: Define ExpandedField and update public API signatures**

Add to `src/expand/mod.rs`:

```rust
/// A field produced by expansion, with per-character quoting tracking.
/// Characters marked as quoted are NOT subject to field splitting or glob.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedField {
    pub value: String,
    /// For each byte in `value`: true = quoted (protected from field split + glob)
    pub quoted_mask: Vec<bool>,
}

impl ExpandedField {
    pub fn new() -> Self {
        Self { value: String::new(), quoted_mask: Vec::new() }
    }

    /// Append text that IS quoted (literal, single-quoted, etc.)
    pub fn push_quoted(&mut self, s: &str) {
        self.value.push_str(s);
        self.quoted_mask.extend(std::iter::repeat(true).take(s.len()));
    }

    /// Append text that is NOT quoted (from expansion results)
    pub fn push_unquoted(&mut self, s: &str) {
        self.value.push_str(s);
        self.quoted_mask.extend(std::iter::repeat(false).take(s.len()));
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}
```

Update the public API to take `&mut ShellEnv` (command sub and ${:=} modify env):

```rust
/// Full word expansion: expand → field split → pathname expand → quote remove.
/// Returns zero or more strings (field splitting can produce multiple fields).
pub fn expand_word(env: &mut ShellEnv, word: &Word) -> Vec<String> {
    let fields = expand_word_to_fields(env, word);
    let fields = field_split::split(env, fields);
    let fields = pathname::expand(env, fields);
    // Quote removal: just extract values
    fields.into_iter()
        .filter(|f| !f.is_empty())
        .map(|f| f.value)
        .collect()
}

/// Expand multiple words, concatenating all resulting fields.
pub fn expand_words(env: &mut ShellEnv, words: &[Word]) -> Vec<String> {
    let mut result = Vec::new();
    for word in words {
        result.extend(expand_word(env, word));
    }
    result
}

/// Expand for assignment value context (no field split, no pathname expansion).
pub fn expand_word_to_string(env: &mut ShellEnv, word: &Word) -> String {
    let fields = expand_word_to_fields(env, word);
    fields.into_iter().map(|f| f.value).collect::<Vec<_>>().join(" ")
}
```

- [ ] **Step 2: Implement expand_word_to_fields with ExpandedField tracking**

```rust
/// Stage 1: Expand all parts of a word into ExpandedFields.
/// Most parts append to a single field. "$@" in double quotes can split.
fn expand_word_to_fields(env: &mut ShellEnv, word: &Word) -> Vec<ExpandedField> {
    let mut fields = vec![ExpandedField::new()];
    for part in &word.parts {
        expand_part_to_fields(env, part, &mut fields, false);
    }
    fields
}

/// Expand a single WordPart, appending to the last field in `fields`.
/// `in_double_quote`: true when inside DoubleQuoted context.
fn expand_part_to_fields(
    env: &mut ShellEnv,
    part: &WordPart,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) {
    match part {
        WordPart::Literal(s) => {
            fields.last_mut().unwrap().push_quoted(s);
        }
        WordPart::SingleQuoted(s) => {
            fields.last_mut().unwrap().push_quoted(s);
        }
        WordPart::DollarSingleQuoted(s) => {
            fields.last_mut().unwrap().push_quoted(s);
        }
        WordPart::DoubleQuoted(parts) => {
            for inner in parts {
                expand_part_to_fields(env, inner, fields, true);
            }
        }
        WordPart::Tilde(user) => {
            let expanded = expand_tilde(env, user.as_deref());
            // Tilde result is protected from field split + glob
            fields.last_mut().unwrap().push_quoted(&expanded);
        }
        WordPart::Parameter(param) => {
            expand_param_to_fields(env, param, fields, in_double_quote);
        }
        WordPart::CommandSub(program) => {
            let output = command_sub::execute(env, program);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&output);
            } else {
                fields.last_mut().unwrap().push_unquoted(&output);
            }
        }
        WordPart::ArithSub(expr) => {
            let result = arith::evaluate(env, expr);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&result);
            } else {
                fields.last_mut().unwrap().push_unquoted(&result);
            }
        }
    }
}
```

- [ ] **Step 3: Implement expand_param_to_fields with $@ handling**

```rust
fn expand_param_to_fields(
    env: &mut ShellEnv,
    param: &ParamExpr,
    fields: &mut Vec<ExpandedField>,
    in_double_quote: bool,
) {
    match param {
        ParamExpr::Special(SpecialParam::At) if in_double_quote => {
            // "$@": each positional param becomes a separate field
            if env.positional_params.is_empty() {
                return; // Zero fields
            }
            for (i, p) in env.positional_params.iter().enumerate() {
                if i == 0 {
                    // Append to current field (handles "prefix$@" → "prefixarg1")
                    fields.last_mut().unwrap().push_quoted(p);
                } else {
                    fields.push(ExpandedField::new());
                    fields.last_mut().unwrap().push_quoted(p);
                }
            }
            // Note: suffix after $@ will append to the last field
        }
        ParamExpr::Special(SpecialParam::Star) if in_double_quote => {
            // "$*": join all params with first char of IFS
            let ifs = env.vars.get("IFS").unwrap_or(" \t\n");
            let sep = ifs.chars().next().unwrap_or(' ');
            let joined = env.positional_params.join(&sep.to_string());
            fields.last_mut().unwrap().push_quoted(&joined);
        }
        _ => {
            // All other param forms produce a single string
            let expanded = param::expand(env, param);
            if in_double_quote {
                fields.last_mut().unwrap().push_quoted(&expanded);
            } else {
                fields.last_mut().unwrap().push_unquoted(&expanded);
            }
        }
    }
}
```

- [ ] **Step 4: Implement expand_tilde**

```rust
fn expand_tilde(env: &ShellEnv, user: Option<&str>) -> String {
    match user {
        None => {
            // ~ → $HOME
            env.vars.get("HOME").unwrap_or("~").to_string()
        }
        Some(username) => {
            // ~user → getpwnam lookup
            #[cfg(unix)]
            {
                use std::ffi::CString;
                if let Ok(c_name) = CString::new(username) {
                    let pw = unsafe { libc::getpwnam(c_name.as_ptr()) };
                    if !pw.is_null() {
                        let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
                        if let Ok(s) = dir.to_str() {
                            return s.to_string();
                        }
                    }
                }
            }
            format!("~{}", username)
        }
    }
}
```

- [ ] **Step 5: Create stub submodules**

Create stub files that will be implemented in later tasks:

`src/expand/param.rs`:
```rust
use crate::env::ShellEnv;
use crate::parser::ast::ParamExpr;

/// Expand a parameter expression to a string.
pub fn expand(env: &mut ShellEnv, param: &ParamExpr) -> String {
    // Implemented in Task 2; for now, forward to legacy logic
    let mut out = String::new();
    crate::expand::expand_param_legacy(env, param, &mut out);
    out
}
```

`src/expand/command_sub.rs`:
```rust
use crate::env::ShellEnv;
use crate::parser::ast::Program;

/// Execute command substitution: run program, capture stdout, strip trailing newlines.
pub fn execute(env: &mut ShellEnv, _program: &Program) -> String {
    // Implemented in Task 3
    String::new()
}
```

`src/expand/arith.rs`:
```rust
use crate::env::ShellEnv;

/// Evaluate an arithmetic expression. Returns the result as a string.
pub fn evaluate(env: &mut ShellEnv, _expr: &str) -> String {
    // Implemented in Task 4
    String::from("0")
}
```

`src/expand/field_split.rs`:
```rust
use crate::env::ShellEnv;
use super::ExpandedField;

/// Split fields based on IFS.
pub fn split(env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    // Implemented in Task 5; pass through for now
    fields
}
```

`src/expand/pathname.rs`:
```rust
use crate::env::ShellEnv;
use super::ExpandedField;

/// Expand pathname patterns (glob) in fields.
pub fn expand(env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    // Implemented in Task 6; pass through for now
    fields
}
```

`src/expand/pattern.rs`:
```rust
/// Match a string against a POSIX shell pattern.
/// Patterns: * (any string), ? (any char), [...] (bracket expr), literal chars.
pub fn matches(pattern: &str, string: &str) -> bool {
    // Implemented in Task 2
    false
}
```

- [ ] **Step 6: Move legacy expand_param logic to a function accessible by param.rs stub**

Rename the existing `expand_param` to `expand_param_legacy` and make it `pub(crate)`:

```rust
pub(crate) fn expand_param_legacy(env: &ShellEnv, param: &ParamExpr, out: &mut String) {
    // ... existing Phase 2 logic ...
}
```

- [ ] **Step 7: Update module declarations**

Update `src/expand/mod.rs` top:
```rust
pub mod arith;
pub mod command_sub;
pub mod field_split;
pub mod param;
pub mod pathname;
pub mod pattern;
```

- [ ] **Step 8: Update executor to use &mut ShellEnv**

In `src/exec/mod.rs`, update `expand_words` calls: the function now takes `&mut self.env` instead of `&self.env`. Also update `expand_word_to_string` calls in `src/exec/redirect.rs`.

Since the executor already has `&mut self`, this should work with the borrow checker. For redirect.rs, the `apply` method will need to take `&mut ShellEnv` instead of `&ShellEnv`.

- [ ] **Step 9: Write tests and verify**

Add tests to `src/expand/mod.rs` tests section:

```rust
    #[test]
    fn test_expand_word_produces_vec() {
        let mut env = make_env();
        env.vars.set("FOO", "hello").unwrap();
        let word = Word { parts: vec![WordPart::Parameter(ParamExpr::Simple("FOO".to_string()))] };
        let result = expand_word(&mut env, &word);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn test_dollar_at_in_double_quotes() {
        let mut env = ShellEnv::new("kish", vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let word = Word { parts: vec![WordPart::DoubleQuoted(vec![
            WordPart::Parameter(ParamExpr::Special(SpecialParam::At)),
        ])] };
        let result = expand_word(&mut env, &word);
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_dollar_at_empty_params() {
        let mut env = ShellEnv::new("kish", vec![]);
        let word = Word { parts: vec![WordPart::DoubleQuoted(vec![
            WordPart::Parameter(ParamExpr::Special(SpecialParam::At)),
        ])] };
        let result = expand_word(&mut env, &word);
        assert!(result.is_empty());
    }

    #[test]
    fn test_dollar_star_in_double_quotes() {
        let mut env = ShellEnv::new("kish", vec!["a".to_string(), "b".to_string()]);
        let word = Word { parts: vec![WordPart::DoubleQuoted(vec![
            WordPart::Parameter(ParamExpr::Special(SpecialParam::Star)),
        ])] };
        let result = expand_word(&mut env, &word);
        assert_eq!(result, vec!["a b"]); // joined by first char of IFS (space)
    }

    #[test]
    fn test_tilde_user_lookup() {
        let mut env = make_env();
        let word = Word { parts: vec![WordPart::Tilde(Some("root".to_string()))] };
        let result = expand_word_to_string(&mut env, &word);
        // On most systems, root's home is /root or /var/root
        assert!(result.starts_with('/') || result == "~root");
    }
```

Run: `cargo test`
Expected: All pass.

- [ ] **Step 10: Commit**

```bash
git add src/expand/ src/exec/
git commit -m "feat(phase3): refactor expansion pipeline with ExpandedField, \"$@\" support, stubs"
```

---

### Task 2: Complete parameter expansion + pattern matching

**Files:**
- Modify: `src/expand/param.rs`
- Modify: `src/expand/pattern.rs`

- [ ] **Step 1: Implement pattern matching**

Replace `src/expand/pattern.rs`:

```rust
/// Match a string against a POSIX shell pattern.
/// Pattern chars: * (any string), ? (any single char), [chars] (bracket expr).
/// Backslash escapes the next character.
pub fn matches(pattern: &str, string: &str) -> bool {
    match_impl(pattern.as_bytes(), string.as_bytes())
}

fn match_impl(pat: &[u8], s: &[u8]) -> bool {
    let mut pi = 0;
    let mut si = 0;
    let mut star_pi = usize::MAX;
    let mut star_si = usize::MAX;

    while si < s.len() {
        if pi < pat.len() {
            match pat[pi] {
                b'?' => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                b'*' => {
                    star_pi = pi;
                    star_si = si;
                    pi += 1;
                    continue;
                }
                b'[' => {
                    if let Some((matched, new_pi)) = match_bracket(&pat[pi..], s[si]) {
                        if matched {
                            pi += new_pi;
                            si += 1;
                            continue;
                        }
                    }
                    // No match — try backtrack
                    if star_pi != usize::MAX {
                        pi = star_pi + 1;
                        star_si += 1;
                        si = star_si;
                        continue;
                    }
                    return false;
                }
                b'\\' if pi + 1 < pat.len() => {
                    if pat[pi + 1] == s[si] {
                        pi += 2;
                        si += 1;
                        continue;
                    }
                    if star_pi != usize::MAX {
                        pi = star_pi + 1;
                        star_si += 1;
                        si = star_si;
                        continue;
                    }
                    return false;
                }
                c => {
                    if c == s[si] {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                    if star_pi != usize::MAX {
                        pi = star_pi + 1;
                        star_si += 1;
                        si = star_si;
                        continue;
                    }
                    return false;
                }
            }
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    // Consume trailing *s
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Match a bracket expression [chars] or [!chars].
/// Returns (matched, bytes_consumed_from_pattern) or None if invalid.
fn match_bracket(pat: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pat.is_empty() || pat[0] != b'[' {
        return None;
    }
    let mut i = 1;
    let negated = if i < pat.len() && pat[i] == b'!' {
        i += 1;
        true
    } else {
        false
    };

    let mut matched = false;
    let mut first = true;

    while i < pat.len() {
        if pat[i] == b']' && !first {
            return Some((matched ^ negated, i + 1));
        }
        first = false;

        let c = pat[i];
        // Range: a-z
        if i + 2 < pat.len() && pat[i + 1] == b'-' && pat[i + 2] != b']' {
            let lo = c;
            let hi = pat[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if c == ch {
                matched = true;
            }
            i += 1;
        }
    }
    None // Unterminated bracket
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn test_literal() { assert!(matches("hello", "hello")); }
    #[test] fn test_literal_no_match() { assert!(!matches("hello", "world")); }
    #[test] fn test_star() { assert!(matches("*.txt", "file.txt")); }
    #[test] fn test_star_all() { assert!(matches("*", "anything")); }
    #[test] fn test_question() { assert!(matches("?.txt", "a.txt")); }
    #[test] fn test_question_no_match() { assert!(!matches("?.txt", "ab.txt")); }
    #[test] fn test_bracket() { assert!(matches("[abc]", "b")); }
    #[test] fn test_bracket_no_match() { assert!(!matches("[abc]", "d")); }
    #[test] fn test_bracket_range() { assert!(matches("[a-z]", "m")); }
    #[test] fn test_bracket_negated() { assert!(matches("[!abc]", "d")); }
    #[test] fn test_bracket_negated_no_match() { assert!(!matches("[!abc]", "a")); }
    #[test] fn test_complex() { assert!(matches("test_*.[ch]", "test_foo.c")); }
    #[test] fn test_backslash_escape() { assert!(matches("\\*", "*")); }
    #[test] fn test_empty_pattern_empty_string() { assert!(matches("", "")); }
    #[test] fn test_star_empty() { assert!(matches("*", "")); }
}
```

- [ ] **Step 2: Implement full parameter expansion**

Replace `src/expand/param.rs`:

```rust
use crate::env::ShellEnv;
use crate::parser::ast::{ParamExpr, SpecialParam, Word};
use crate::expand::pattern;

/// Expand a parameter expression to a string.
pub fn expand(env: &mut ShellEnv, param: &ParamExpr) -> String {
    match param {
        ParamExpr::Simple(name) => {
            env.vars.get(name).unwrap_or("").to_string()
        }
        ParamExpr::Positional(n) => {
            if *n > 0 {
                env.positional_params.get(*n - 1).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        }
        ParamExpr::Special(sp) => expand_special(env, sp),
        ParamExpr::Length(name) => {
            let val = get_param_value(env, name);
            val.map(|v| v.len().to_string()).unwrap_or_else(|| "0".to_string())
        }
        ParamExpr::Default { name, word, null_check } => {
            let val = get_param_value(env, name);
            if is_unset_or_null(&val, *null_check) {
                word.as_ref().map(|w| crate::expand::expand_word_to_string(env, w)).unwrap_or_default()
            } else {
                val.unwrap_or_default()
            }
        }
        ParamExpr::Assign { name, word, null_check } => {
            let val = get_param_value(env, name);
            if is_unset_or_null(&val, *null_check) {
                let new_val = word.as_ref().map(|w| crate::expand::expand_word_to_string(env, w)).unwrap_or_default();
                let _ = env.vars.set(name, new_val.clone());
                new_val
            } else {
                val.unwrap_or_default()
            }
        }
        ParamExpr::Error { name, word, null_check } => {
            let val = get_param_value(env, name);
            if is_unset_or_null(&val, *null_check) {
                let msg = word.as_ref()
                    .map(|w| crate::expand::expand_word_to_string(env, w))
                    .unwrap_or_else(|| format!("{}: parameter not set", name));
                eprintln!("kish: {}: {}", name, msg);
                // In a real shell, this would cause the script to exit.
                // For now, return empty.
                String::new()
            } else {
                val.unwrap_or_default()
            }
        }
        ParamExpr::Alt { name, word, null_check } => {
            let val = get_param_value(env, name);
            if is_unset_or_null(&val, *null_check) {
                String::new()
            } else {
                word.as_ref().map(|w| crate::expand::expand_word_to_string(env, w)).unwrap_or_default()
            }
        }
        ParamExpr::StripShortSuffix(name, pat_word) => {
            let val = get_param_value(env, name).unwrap_or_default();
            let pat = crate::expand::expand_word_to_string(env, pat_word);
            strip_suffix(&val, &pat, false)
        }
        ParamExpr::StripLongSuffix(name, pat_word) => {
            let val = get_param_value(env, name).unwrap_or_default();
            let pat = crate::expand::expand_word_to_string(env, pat_word);
            strip_suffix(&val, &pat, true)
        }
        ParamExpr::StripShortPrefix(name, pat_word) => {
            let val = get_param_value(env, name).unwrap_or_default();
            let pat = crate::expand::expand_word_to_string(env, pat_word);
            strip_prefix(&val, &pat, false)
        }
        ParamExpr::StripLongPrefix(name, pat_word) => {
            let val = get_param_value(env, name).unwrap_or_default();
            let pat = crate::expand::expand_word_to_string(env, pat_word);
            strip_prefix(&val, &pat, true)
        }
    }
}

fn get_param_value(env: &ShellEnv, name: &str) -> Option<String> {
    env.vars.get(name).map(|v| v.to_string())
}

fn is_unset_or_null(val: &Option<String>, null_check: bool) -> bool {
    match val {
        None => true,
        Some(v) if null_check && v.is_empty() => true,
        _ => false,
    }
}

fn expand_special(env: &ShellEnv, sp: &SpecialParam) -> String {
    match sp {
        SpecialParam::Question => env.last_exit_status.to_string(),
        SpecialParam::Dollar => env.shell_pid.as_raw().to_string(),
        SpecialParam::Zero => env.shell_name.clone(),
        SpecialParam::Hash => env.positional_params.len().to_string(),
        SpecialParam::At | SpecialParam::Star => {
            // Unquoted $@ and $*: join with space (field splitting handles the rest)
            env.positional_params.join(" ")
        }
        SpecialParam::Bang => {
            env.last_bg_pid.map(|p| p.to_string()).unwrap_or_default()
        }
        SpecialParam::Dash => String::new(), // Phase 7
    }
}

/// Remove the shortest/longest suffix matching pattern.
fn strip_suffix(value: &str, pat: &str, longest: bool) -> String {
    if longest {
        // Try from the beginning (longest suffix = shortest remaining prefix)
        for i in 0..=value.len() {
            if pattern::matches(pat, &value[i..]) {
                return value[..i].to_string();
            }
        }
    } else {
        // Try from the end (shortest suffix)
        for i in (0..=value.len()).rev() {
            if pattern::matches(pat, &value[i..]) {
                return value[..i].to_string();
            }
        }
    }
    value.to_string()
}

/// Remove the shortest/longest prefix matching pattern.
fn strip_prefix(value: &str, pat: &str, longest: bool) -> String {
    if longest {
        // Try from the end (longest prefix = shortest remaining suffix)
        for i in (0..=value.len()).rev() {
            if pattern::matches(pat, &value[..i]) {
                return value[i..].to_string();
            }
        }
    } else {
        // Try from the beginning (shortest prefix)
        for i in 0..=value.len() {
            if pattern::matches(pat, &value[..i]) {
                return value[i..].to_string();
            }
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env() -> ShellEnv {
        let mut env = ShellEnv::new("kish", vec![]);
        env.vars.set("FOO", "hello").unwrap();
        env.vars.set("FILE", "/path/to/file.txt").unwrap();
        env.vars.set("EMPTY", "").unwrap();
        env
    }

    #[test] fn test_simple() {
        let mut env = test_env();
        assert_eq!(expand(&mut env, &ParamExpr::Simple("FOO".to_string())), "hello");
    }

    #[test] fn test_assign_unset() {
        let mut env = test_env();
        let result = expand(&mut env, &ParamExpr::Assign {
            name: "NEW".to_string(),
            word: Some(Word::literal("assigned")),
            null_check: false,
        });
        assert_eq!(result, "assigned");
        assert_eq!(env.vars.get("NEW"), Some("assigned"));
    }

    #[test] fn test_assign_already_set() {
        let mut env = test_env();
        let result = expand(&mut env, &ParamExpr::Assign {
            name: "FOO".to_string(),
            word: Some(Word::literal("new")),
            null_check: false,
        });
        assert_eq!(result, "hello"); // Not overwritten
    }

    #[test] fn test_alt_set() {
        let mut env = test_env();
        assert_eq!(expand(&mut env, &ParamExpr::Alt {
            name: "FOO".to_string(), word: Some(Word::literal("alt")), null_check: false,
        }), "alt");
    }

    #[test] fn test_alt_unset() {
        let mut env = test_env();
        assert_eq!(expand(&mut env, &ParamExpr::Alt {
            name: "NOPE".to_string(), word: Some(Word::literal("alt")), null_check: false,
        }), "");
    }

    #[test] fn test_strip_short_suffix() {
        assert_eq!(strip_suffix("/path/to/file.txt", ".*", false), "/path/to/file");
    }

    #[test] fn test_strip_long_suffix() {
        assert_eq!(strip_suffix("/path/to/file.txt", ".*", true), "/path/to/file");
    }

    #[test] fn test_strip_short_prefix() {
        assert_eq!(strip_prefix("/path/to/file.txt", "*/", false), "path/to/file.txt");
    }

    #[test] fn test_strip_long_prefix() {
        assert_eq!(strip_prefix("/path/to/file.txt", "*/", true), "file.txt");
    }

    #[test] fn test_strip_percent() {
        let mut env = test_env();
        assert_eq!(expand(&mut env, &ParamExpr::StripShortSuffix(
            "FILE".to_string(), Word::literal(".*"),
        )), "/path/to/file");
    }

    #[test] fn test_strip_hash() {
        let mut env = test_env();
        assert_eq!(expand(&mut env, &ParamExpr::StripLongPrefix(
            "FILE".to_string(), Word::literal("*/"),
        )), "file.txt");
    }

    #[test] fn test_length() {
        let mut env = test_env();
        assert_eq!(expand(&mut env, &ParamExpr::Length("FOO".to_string())), "5");
    }
}
```

- [ ] **Step 3: Remove expand_param_legacy from mod.rs**

Once param.rs is working, remove the `expand_param_legacy` function and update `expand_param_to_fields` to call `param::expand` directly.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/expand/
git commit -m "feat(phase3): complete parameter expansion + pattern matching"
```

---

### Task 3: Command substitution

**Files:**
- Modify: `src/expand/command_sub.rs`

- [ ] **Step 1: Implement command substitution**

Replace `src/expand/command_sub.rs`:

```rust
use std::io::Read;
use std::os::fd::RawFd;

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{close, fork, ForkResult};

use crate::env::ShellEnv;
use crate::exec::Executor;
use crate::parser::ast::Program;

/// Execute command substitution: run program in subshell, capture stdout.
/// Strips trailing newlines from the output.
pub fn execute(env: &mut ShellEnv, program: &Program) -> String {
    // Create a pipe to capture stdout
    let mut fds: [i32; 2] = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
        eprintln!("kish: pipe: {}", std::io::Error::last_os_error());
        return String::new();
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Child: redirect stdout to pipe write end
            unsafe { libc::close(read_fd) };
            if unsafe { libc::dup2(write_fd, 1) } == -1 {
                unsafe { libc::_exit(1) };
            }
            unsafe { libc::close(write_fd) };

            // Execute the program
            let mut executor = Executor::new(env.shell_name.clone(), env.positional_params.clone());
            // Copy variables
            executor.env.vars = env.vars.clone();
            executor.env.last_exit_status = env.last_exit_status;

            let status = executor.exec_program(program);
            std::process::exit(status);
        }
        Ok(ForkResult::Parent { child }) => {
            unsafe { libc::close(write_fd) };

            // Read all output from pipe
            let mut output = String::new();
            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
            let _ = file.read_to_string(&mut output);

            // Wait for child
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(_, code)) => {
                    env.last_exit_status = code;
                }
                Ok(WaitStatus::Signaled(_, sig, _)) => {
                    env.last_exit_status = 128 + sig as i32;
                }
                _ => {}
            }

            // Strip trailing newlines (POSIX requirement)
            while output.ends_with('\n') {
                output.pop();
            }

            output
        }
        Err(e) => {
            unsafe { libc::close(read_fd) };
            unsafe { libc::close(write_fd) };
            eprintln!("kish: fork: {}", e);
            String::new()
        }
    }
}

use std::os::fd::FromRawFd;
```

- [ ] **Step 2: Add integration tests**

Add to `tests/parser_integration.rs`:

```rust
#[test]
fn test_command_substitution() {
    let out = kish_exec("echo $(echo hello)");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_command_substitution_strips_trailing_newlines() {
    let out = kish_exec("echo \"x$(echo hello)x\"");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "xhellox\n");
}

#[test]
fn test_nested_command_substitution() {
    let out = kish_exec("echo $(echo $(echo deep))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "deep\n");
}

#[test]
fn test_command_sub_exit_status() {
    let out = kish_exec("x=$(false); echo $?");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/expand/command_sub.rs tests/parser_integration.rs
git commit -m "feat(phase3): command substitution with fork/pipe/exec"
```

---

### Task 4: Arithmetic expansion

**Files:**
- Modify: `src/expand/arith.rs`

- [ ] **Step 1: Implement arithmetic lexer, parser, and evaluator**

Replace `src/expand/arith.rs`:

```rust
use crate::env::ShellEnv;

/// Evaluate an arithmetic expression string. Returns result as string.
/// On error, prints to stderr and returns "0".
pub fn evaluate(env: &mut ShellEnv, expr: &str) -> String {
    // First, expand $VAR references in the expression
    let expanded = expand_vars_in_expr(env, expr);

    match parse_and_eval(env, &expanded) {
        Ok(val) => val.to_string(),
        Err(e) => {
            eprintln!("kish: arithmetic: {}", e);
            "0".to_string()
        }
    }
}

/// Replace $NAME and ${NAME} references in arithmetic expression with their values.
fn expand_vars_in_expr(env: &ShellEnv, expr: &str) -> String {
    let mut result = String::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'{' {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' { i += 1; }
                let name = &expr[start..i];
                result.push_str(env.vars.get(name).unwrap_or("0"));
                if i < bytes.len() { i += 1; } // skip }
            } else {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                let name = &expr[start..i];
                if !name.is_empty() {
                    result.push_str(env.vars.get(name).unwrap_or("0"));
                } else {
                    result.push('$');
                }
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

// --- Recursive descent parser/evaluator ---

struct ArithParser<'a> {
    input: &'a [u8],
    pos: usize,
    env: &'a mut ShellEnv,
}

fn parse_and_eval(env: &mut ShellEnv, expr: &str) -> Result<i64, String> {
    let mut parser = ArithParser { input: expr.trim().as_bytes(), pos: 0, env };
    let result = parser.expr()?;
    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        return Err(format!("unexpected character at position {}", parser.pos));
    }
    Ok(result)
}

impl<'a> ArithParser<'a> {
    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_whitespace();
        self.input.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.input.get(self.pos + 1).copied()
    }

    // expr: ternary
    fn expr(&mut self) -> Result<i64, String> {
        self.ternary()
    }

    // ternary: logical_or ? expr : expr
    fn ternary(&mut self) -> Result<i64, String> {
        let cond = self.logical_or()?;
        self.skip_whitespace();
        if self.peek() == Some(b'?') {
            self.pos += 1;
            let then_val = self.expr()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err("expected ':' in ternary".to_string());
            }
            self.pos += 1;
            let else_val = self.expr()?;
            Ok(if cond != 0 { then_val } else { else_val })
        } else {
            Ok(cond)
        }
    }

    fn logical_or(&mut self) -> Result<i64, String> {
        let mut left = self.logical_and()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len() && self.input[self.pos] == b'|' && self.input[self.pos + 1] == b'|' {
                self.pos += 2;
                let right = self.logical_and()?;
                left = if left != 0 || right != 0 { 1 } else { 0 };
            } else { break; }
        }
        Ok(left)
    }

    fn logical_and(&mut self) -> Result<i64, String> {
        let mut left = self.bitwise_or()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len() && self.input[self.pos] == b'&' && self.input[self.pos + 1] == b'&' {
                self.pos += 2;
                let right = self.bitwise_or()?;
                left = if left != 0 && right != 0 { 1 } else { 0 };
            } else { break; }
        }
        Ok(left)
    }

    fn bitwise_or(&mut self) -> Result<i64, String> {
        let mut left = self.bitwise_xor()?;
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b'|') && self.peek2() != Some(b'|') {
                self.pos += 1;
                let right = self.bitwise_xor()?;
                left |= right;
            } else { break; }
        }
        Ok(left)
    }

    fn bitwise_xor(&mut self) -> Result<i64, String> {
        let mut left = self.bitwise_and()?;
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b'^') {
                self.pos += 1;
                let right = self.bitwise_and()?;
                left ^= right;
            } else { break; }
        }
        Ok(left)
    }

    fn bitwise_and(&mut self) -> Result<i64, String> {
        let mut left = self.equality()?;
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b'&') && self.peek2() != Some(b'&') {
                self.pos += 1;
                let right = self.equality()?;
                left &= right;
            } else { break; }
        }
        Ok(left)
    }

    fn equality(&mut self) -> Result<i64, String> {
        let mut left = self.relational()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len() {
                if self.input[self.pos] == b'=' && self.input[self.pos + 1] == b'=' {
                    self.pos += 2;
                    let right = self.relational()?;
                    left = if left == right { 1 } else { 0 };
                } else if self.input[self.pos] == b'!' && self.input[self.pos + 1] == b'=' {
                    self.pos += 2;
                    let right = self.relational()?;
                    left = if left != right { 1 } else { 0 };
                } else { break; }
            } else { break; }
        }
        Ok(left)
    }

    fn relational(&mut self) -> Result<i64, String> {
        let mut left = self.shift()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len() && self.input[self.pos] == b'<' && self.input[self.pos + 1] == b'=' {
                self.pos += 2;
                let right = self.shift()?;
                left = if left <= right { 1 } else { 0 };
            } else if self.pos + 1 < self.input.len() && self.input[self.pos] == b'>' && self.input[self.pos + 1] == b'=' {
                self.pos += 2;
                let right = self.shift()?;
                left = if left >= right { 1 } else { 0 };
            } else if self.peek() == Some(b'<') && self.peek2() != Some(b'<') {
                self.pos += 1;
                let right = self.shift()?;
                left = if left < right { 1 } else { 0 };
            } else if self.peek() == Some(b'>') && self.peek2() != Some(b'>') {
                self.pos += 1;
                let right = self.shift()?;
                left = if left > right { 1 } else { 0 };
            } else { break; }
        }
        Ok(left)
    }

    fn shift(&mut self) -> Result<i64, String> {
        let mut left = self.additive()?;
        loop {
            self.skip_whitespace();
            if self.pos + 1 < self.input.len() {
                if self.input[self.pos] == b'<' && self.input[self.pos + 1] == b'<' {
                    self.pos += 2;
                    let right = self.additive()?;
                    left <<= right;
                } else if self.input[self.pos] == b'>' && self.input[self.pos + 1] == b'>' {
                    self.pos += 2;
                    let right = self.additive()?;
                    left >>= right;
                } else { break; }
            } else { break; }
        }
        Ok(left)
    }

    fn additive(&mut self) -> Result<i64, String> {
        let mut left = self.multiplicative()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'+') => { self.pos += 1; let right = self.multiplicative()?; left += right; }
                Some(b'-') => { self.pos += 1; let right = self.multiplicative()?; left -= right; }
                _ => break,
            }
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Result<i64, String> {
        let mut left = self.unary()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'*') => { self.pos += 1; let right = self.unary()?; left *= right; }
                Some(b'/') => {
                    self.pos += 1; let right = self.unary()?;
                    if right == 0 { return Err("division by zero".to_string()); }
                    left /= right;
                }
                Some(b'%') => {
                    self.pos += 1; let right = self.unary()?;
                    if right == 0 { return Err("division by zero".to_string()); }
                    left %= right;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<i64, String> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'-') => { self.pos += 1; Ok(-self.unary()?) }
            Some(b'+') => { self.pos += 1; self.unary() }
            Some(b'!') if self.peek2() != Some(b'=') => { self.pos += 1; Ok(if self.unary()? == 0 { 1 } else { 0 }) }
            Some(b'~') => { self.pos += 1; Ok(!self.unary()?) }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<i64, String> {
        self.skip_whitespace();
        if self.peek() == Some(b'(') {
            self.pos += 1;
            let val = self.expr()?;
            self.skip_whitespace();
            if self.peek() != Some(b')') {
                return Err("expected ')'".to_string());
            }
            self.pos += 1;
            return Ok(val);
        }

        // Variable name (bare name in arithmetic context)
        if self.pos < self.input.len() && (self.input[self.pos].is_ascii_alphabetic() || self.input[self.pos] == b'_') {
            let start = self.pos;
            while self.pos < self.input.len() && (self.input[self.pos].is_ascii_alphanumeric() || self.input[self.pos] == b'_') {
                self.pos += 1;
            }
            let name = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");

            // Check for assignment: name = expr
            self.skip_whitespace();
            if self.peek() == Some(b'=') && self.peek2() != Some(b'=') {
                self.pos += 1;
                let val = self.expr()?;
                let _ = self.env.vars.set(name, val.to_string());
                return Ok(val);
            }

            let val_str = self.env.vars.get(name).unwrap_or("0");
            return parse_number(val_str);
        }

        // Number literal
        self.parse_number_literal()
    }

    fn parse_number_literal(&mut self) -> Result<i64, String> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Err("unexpected end of expression".to_string());
        }

        let start = self.pos;
        // Hex: 0x...
        if self.pos + 1 < self.input.len() && self.input[self.pos] == b'0' && (self.input[self.pos + 1] == b'x' || self.input[self.pos + 1] == b'X') {
            self.pos += 2;
            let hex_start = self.pos;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_hexdigit() { self.pos += 1; }
            let hex = std::str::from_utf8(&self.input[hex_start..self.pos]).unwrap_or("0");
            return i64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex: {}", e));
        }
        // Octal: 0...
        if self.input[self.pos] == b'0' {
            self.pos += 1;
            if self.pos < self.input.len() && self.input[self.pos] >= b'0' && self.input[self.pos] <= b'7' {
                let oct_start = self.pos;
                while self.pos < self.input.len() && self.input[self.pos] >= b'0' && self.input[self.pos] <= b'7' { self.pos += 1; }
                let oct = std::str::from_utf8(&self.input[oct_start..self.pos]).unwrap_or("0");
                return i64::from_str_radix(oct, 8).map_err(|e| format!("invalid octal: {}", e));
            }
            return Ok(0);
        }
        // Decimal
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() { self.pos += 1; }
        if self.pos == start {
            return Err(format!("expected number, got '{}'", self.input[self.pos] as char));
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("0");
        s.parse::<i64>().map_err(|e| format!("invalid number: {}", e))
    }
}

fn parse_number(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if s.is_empty() { return Ok(0); }
    if s.starts_with("0x") || s.starts_with("0X") {
        i64::from_str_radix(&s[2..], 16).map_err(|e| e.to_string())
    } else if s.starts_with('0') && s.len() > 1 && s.chars().all(|c| c >= '0' && c <= '7') {
        i64::from_str_radix(&s[1..], 8).map_err(|e| e.to_string())
    } else {
        s.parse::<i64>().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env() -> ShellEnv {
        let mut env = ShellEnv::new("kish", vec![]);
        env.vars.set("x", "10").unwrap();
        env.vars.set("y", "3").unwrap();
        env
    }

    #[test] fn test_simple_number() { assert_eq!(evaluate(&mut test_env(), "42"), "42"); }
    #[test] fn test_addition() { assert_eq!(evaluate(&mut test_env(), "1 + 2"), "3"); }
    #[test] fn test_multiplication() { assert_eq!(evaluate(&mut test_env(), "3 * 4"), "12"); }
    #[test] fn test_precedence() { assert_eq!(evaluate(&mut test_env(), "2 + 3 * 4"), "14"); }
    #[test] fn test_parens() { assert_eq!(evaluate(&mut test_env(), "(2 + 3) * 4"), "20"); }
    #[test] fn test_division() { assert_eq!(evaluate(&mut test_env(), "10 / 3"), "3"); }
    #[test] fn test_modulo() { assert_eq!(evaluate(&mut test_env(), "10 % 3"), "1"); }
    #[test] fn test_unary_minus() { assert_eq!(evaluate(&mut test_env(), "-5"), "-5"); }
    #[test] fn test_comparison() { assert_eq!(evaluate(&mut test_env(), "3 > 2"), "1"); }
    #[test] fn test_equality() { assert_eq!(evaluate(&mut test_env(), "3 == 3"), "1"); }
    #[test] fn test_logical_and() { assert_eq!(evaluate(&mut test_env(), "1 && 0"), "0"); }
    #[test] fn test_logical_or() { assert_eq!(evaluate(&mut test_env(), "0 || 1"), "1"); }
    #[test] fn test_ternary() { assert_eq!(evaluate(&mut test_env(), "1 ? 10 : 20"), "10"); }
    #[test] fn test_bitwise() { assert_eq!(evaluate(&mut test_env(), "5 & 3"), "1"); }
    #[test] fn test_shift() { assert_eq!(evaluate(&mut test_env(), "1 << 4"), "16"); }
    #[test] fn test_hex() { assert_eq!(evaluate(&mut test_env(), "0xFF"), "255"); }
    #[test] fn test_octal() { assert_eq!(evaluate(&mut test_env(), "010"), "8"); }
    #[test] fn test_variable_ref() { assert_eq!(evaluate(&mut test_env(), "x + y"), "13"); }
    #[test] fn test_dollar_variable() { assert_eq!(evaluate(&mut test_env(), "$x + $y"), "13"); }
    #[test] fn test_variable_assign() {
        let mut env = test_env();
        assert_eq!(evaluate(&mut env, "z = 5 + 3"), "8");
        assert_eq!(env.vars.get("z"), Some("8"));
    }
    #[test] fn test_logical_not() { assert_eq!(evaluate(&mut test_env(), "!0"), "1"); }
    #[test] fn test_bitwise_not() { assert_eq!(evaluate(&mut test_env(), "~0"), "-1"); }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/expand/arith.rs
git commit -m "feat(phase3): arithmetic expansion with full C-style operator support"
```

---

### Task 5: Field splitting

**Files:**
- Modify: `src/expand/field_split.rs`

- [ ] **Step 1: Implement IFS field splitting**

Replace `src/expand/field_split.rs`:

```rust
use crate::env::ShellEnv;
use super::ExpandedField;

/// Split fields based on IFS. Only unquoted bytes (quoted_mask=false) from
/// expansion results are subject to splitting.
pub fn split(env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    let ifs = get_ifs(env);

    // IFS empty → no splitting (but remove empty unquoted fields)
    if ifs.is_empty() {
        return fields.into_iter()
            .filter(|f| !f.value.is_empty())
            .collect();
    }

    let ifs_whitespace: Vec<u8> = ifs.bytes()
        .filter(|b| *b == b' ' || *b == b'\t' || *b == b'\n')
        .collect();
    let ifs_non_whitespace: Vec<u8> = ifs.bytes()
        .filter(|b| *b != b' ' && *b != b'\t' && *b != b'\n')
        .collect();

    let mut result = Vec::new();
    for field in fields {
        split_field(&field, &ifs_whitespace, &ifs_non_whitespace, &mut result);
    }
    result
}

fn get_ifs(env: &ShellEnv) -> String {
    // IFS unset → default " \t\n"
    // IFS set to empty → no splitting
    match env.vars.get("IFS") {
        Some(ifs) => ifs.to_string(),
        None => " \t\n".to_string(),
    }
}

fn split_field(
    field: &ExpandedField,
    ifs_ws: &[u8],
    ifs_nws: &[u8],
    result: &mut Vec<ExpandedField>,
) {
    let bytes = field.value.as_bytes();
    let mask = &field.quoted_mask;

    // If all bytes are quoted, no splitting possible
    if mask.iter().all(|&q| q) {
        result.push(field.clone());
        return;
    }

    let mut current = ExpandedField::new();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];
        let quoted = mask.get(i).copied().unwrap_or(true);

        if quoted {
            // Quoted byte: always part of current field
            current.value.push(ch as char);
            current.quoted_mask.push(true);
            i += 1;
            continue;
        }

        let is_ifs_ws = ifs_ws.contains(&ch);
        let is_ifs_nws = ifs_nws.contains(&ch);

        if is_ifs_nws {
            // Non-whitespace IFS delimiter: creates a field boundary
            // Skip surrounding IFS whitespace
            if !current.is_empty() {
                result.push(std::mem::replace(&mut current, ExpandedField::new()));
            } else {
                // Empty field before delimiter
                result.push(ExpandedField::new());
            }
            i += 1;
            // Skip trailing IFS whitespace
            while i < bytes.len() && !mask.get(i).copied().unwrap_or(true) {
                let c = bytes[i];
                if ifs_ws.contains(&c) { i += 1; } else { break; }
            }
        } else if is_ifs_ws {
            // IFS whitespace: skip consecutive whitespace, split
            if !current.is_empty() {
                result.push(std::mem::replace(&mut current, ExpandedField::new()));
            }
            while i < bytes.len() && !mask.get(i).copied().unwrap_or(true) {
                let c = bytes[i];
                if ifs_ws.contains(&c) { i += 1; } else { break; }
            }
        } else {
            current.value.push(ch as char);
            current.quoted_mask.push(false);
            i += 1;
        }
    }

    if !current.is_empty() {
        result.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_ifs(ifs: &str) -> ShellEnv {
        let mut env = ShellEnv::new("kish", vec![]);
        env.vars.set("IFS", ifs).unwrap();
        env
    }

    fn unquoted(s: &str) -> ExpandedField {
        ExpandedField {
            value: s.to_string(),
            quoted_mask: vec![false; s.len()],
        }
    }

    fn quoted(s: &str) -> ExpandedField {
        ExpandedField {
            value: s.to_string(),
            quoted_mask: vec![true; s.len()],
        }
    }

    #[test]
    fn test_split_spaces() {
        let env = env_with_ifs(" \t\n");
        let fields = vec![unquoted("hello world foo")];
        let result = split(&env, fields);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].value, "hello");
        assert_eq!(result[1].value, "world");
        assert_eq!(result[2].value, "foo");
    }

    #[test]
    fn test_split_quoted_not_split() {
        let env = env_with_ifs(" \t\n");
        let fields = vec![quoted("hello world")];
        let result = split(&env, fields);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, "hello world");
    }

    #[test]
    fn test_split_colon_delimiter() {
        let env = env_with_ifs(":");
        let fields = vec![unquoted("a:b:c")];
        let result = split(&env, fields);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].value, "a");
        assert_eq!(result[1].value, "b");
        assert_eq!(result[2].value, "c");
    }

    #[test]
    fn test_empty_ifs_no_split() {
        let env = env_with_ifs("");
        let fields = vec![unquoted("hello world")];
        let result = split(&env, fields);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, "hello world");
    }

    #[test]
    fn test_consecutive_whitespace() {
        let env = env_with_ifs(" \t\n");
        let fields = vec![unquoted("  hello   world  ")];
        let result = split(&env, fields);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].value, "hello");
        assert_eq!(result[1].value, "world");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/expand/field_split.rs
git commit -m "feat(phase3): IFS field splitting with whitespace/non-whitespace distinction"
```

---

### Task 6: Pathname expansion (glob)

**Files:**
- Modify: `src/expand/pathname.rs`

- [ ] **Step 1: Implement pathname expansion**

Replace `src/expand/pathname.rs`:

```rust
use crate::env::ShellEnv;
use super::ExpandedField;
use super::pattern;

/// Expand pathname patterns (glob) in fields.
/// Unquoted *, ?, [ characters trigger glob matching against the filesystem.
pub fn expand(_env: &ShellEnv, fields: Vec<ExpandedField>) -> Vec<ExpandedField> {
    // TODO: check env.options.noglob (set -f) when ShellOptions is implemented
    let mut result = Vec::new();
    for field in fields {
        if has_unquoted_glob_chars(&field) {
            let matches = glob_match(&field.value);
            if matches.is_empty() {
                // No match: keep pattern as-is
                result.push(field);
            } else {
                for m in matches {
                    result.push(ExpandedField {
                        quoted_mask: vec![true; m.len()], // Results are literal
                        value: m,
                    });
                }
            }
        } else {
            result.push(field);
        }
    }
    result
}

/// Check if a field contains unquoted glob metacharacters.
fn has_unquoted_glob_chars(field: &ExpandedField) -> bool {
    let bytes = field.value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if !field.quoted_mask.get(i).copied().unwrap_or(true) {
            if b == b'*' || b == b'?' || b == b'[' {
                return true;
            }
        }
    }
    false
}

/// Match a glob pattern against the filesystem. Returns sorted list of matches.
fn glob_match(pattern_str: &str) -> Vec<String> {
    // Simple case: no directory components with globs
    if !pattern_str.contains('/') {
        return glob_in_dir(".", pattern_str);
    }

    // Split into directory components and expand each
    let parts: Vec<&str> = pattern_str.split('/').collect();
    let mut paths = vec![if pattern_str.starts_with('/') {
        "/".to_string()
    } else {
        ".".to_string()
    }];

    let start = if pattern_str.starts_with('/') { 1 } else { 0 };

    for (idx, part) in parts[start..].iter().enumerate() {
        if part.is_empty() { continue; }
        let mut next_paths = Vec::new();
        for base in &paths {
            if has_glob_meta(part) {
                let matches = glob_in_dir(base, part);
                for m in matches {
                    let path = if base == "." {
                        m
                    } else if base == "/" {
                        format!("/{}", m)
                    } else {
                        format!("{}/{}", base, m)
                    };
                    next_paths.push(path);
                }
            } else {
                let path = if base == "." {
                    part.to_string()
                } else if base == "/" {
                    format!("/{}", part)
                } else {
                    format!("{}/{}", base, part)
                };
                if std::path::Path::new(&path).exists() || idx < parts.len() - start - 1 {
                    next_paths.push(path);
                }
            }
        }
        paths = next_paths;
    }

    // Remove "./" prefix if pattern didn't start with "./"
    if !pattern_str.starts_with("./") {
        paths = paths.iter().map(|p| {
            p.strip_prefix("./").unwrap_or(p).to_string()
        }).collect();
    }

    paths.sort();
    paths
}

fn glob_in_dir(dir: &str, pat: &str) -> Vec<String> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut matches: Vec<String> = read_dir
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            // Leading dot: pattern must start with explicit dot
            if name.starts_with('.') && !pat.starts_with('.') {
                return None;
            }
            if pattern::matches(pat, &name) {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    matches.sort();
    matches
}

fn has_glob_meta(s: &str) -> bool {
    s.bytes().any(|b| b == b'*' || b == b'?' || b == b'[')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_glob_passthrough() {
        let env = ShellEnv::new("kish", vec![]);
        let field = ExpandedField {
            value: "hello".to_string(),
            quoted_mask: vec![false; 5],
        };
        let result = expand(&env, vec![field]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, "hello");
    }

    #[test]
    fn test_quoted_glob_not_expanded() {
        let env = ShellEnv::new("kish", vec![]);
        let field = ExpandedField {
            value: "*.rs".to_string(),
            quoted_mask: vec![true; 4], // All quoted
        };
        let result = expand(&env, vec![field]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, "*.rs");
    }

    #[test]
    fn test_glob_src_files() {
        let env = ShellEnv::new("kish", vec![]);
        let field = ExpandedField {
            value: "src/*.rs".to_string(),
            quoted_mask: vec![false; 8],
        };
        let result = expand(&env, vec![field]);
        // Should find at least main.rs and error.rs
        let values: Vec<&str> = result.iter().map(|f| f.value.as_str()).collect();
        assert!(values.contains(&"src/main.rs"), "expected src/main.rs in {:?}", values);
        assert!(values.contains(&"src/error.rs"), "expected src/error.rs in {:?}", values);
    }

    #[test]
    fn test_no_match_keeps_pattern() {
        let env = ShellEnv::new("kish", vec![]);
        let field = ExpandedField {
            value: "nonexistent_*.xyz".to_string(),
            quoted_mask: vec![false; 18],
        };
        let result = expand(&env, vec![field]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, "nonexistent_*.xyz");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/expand/pathname.rs
git commit -m "feat(phase3): pathname expansion (glob) with dot/slash rules"
```

---

### Task 7: Integration — wire into executor + comprehensive tests

**Files:**
- Modify: `src/exec/mod.rs`
- Modify: `src/exec/redirect.rs`
- Modify: `tests/parser_integration.rs`

- [ ] **Step 1: Update executor to use new expansion API**

In `src/exec/mod.rs`, the `expand_words` call already works since we updated the signatures in Task 1. Verify that `exec_simple_command` correctly uses `&mut self.env` for expansion calls.

Also update `expand_word_to_string` calls in `exec_simple_command` for assignment values:

```rust
let value = assign.value.as_ref()
    .map(|w| crate::expand::expand_word_to_string(&mut self.env, w))
    .unwrap_or_default();
```

- [ ] **Step 2: Update redirect.rs to use &mut ShellEnv**

In `src/exec/redirect.rs`, update the `apply` method signature to take `&mut ShellEnv` and pass `env` mutably to `expand_word_to_string`.

- [ ] **Step 3: Add comprehensive integration tests**

Add to `tests/parser_integration.rs`:

```rust
// --- Phase 3: Expansion tests ---

#[test]
fn test_field_splitting() {
    let out = kish_exec("x='a b c'; for w in $x; do echo $w; done");
    // Field splitting is Phase 3, for loop is Phase 5.
    // Until Phase 5, test with echo directly.
    // Test that unquoted expansion splits:
    let out = kish_exec("x='hello world'; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello world\n");
}

#[test]
fn test_arithmetic_expansion() {
    let out = kish_exec("echo $((2 + 3 * 4))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "14\n");
}

#[test]
fn test_arithmetic_with_variables() {
    let out = kish_exec("x=10; y=3; echo $((x + y))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "13\n");
}

#[test]
fn test_arithmetic_hex() {
    let out = kish_exec("echo $((0xFF))");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "255\n");
}

#[test]
fn test_param_assign() {
    let out = kish_exec("echo ${x:=hello}; echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nhello\n");
}

#[test]
fn test_param_alt() {
    let out = kish_exec("x=set; echo ${x:+alt}; echo ${y:+alt}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "alt\n\n");
}

#[test]
fn test_param_strip_suffix() {
    let out = kish_exec("f=/path/to/file.txt; echo ${f%.txt}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "/path/to/file\n");
}

#[test]
fn test_param_strip_long_prefix() {
    let out = kish_exec("f=/path/to/file.txt; echo ${f##*/}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "file.txt\n");
}

#[test]
fn test_param_length() {
    let out = kish_exec("x=hello; echo ${#x}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn test_command_sub_in_assignment() {
    let out = kish_exec("x=$(echo hello); echo $x");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n");
}

#[test]
fn test_glob_expansion() {
    // This should expand to at least src/main.rs
    let out = kish_exec("echo src/main.rs");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "src/main.rs");
}

#[test]
fn test_quoted_glob_no_expansion() {
    let out = kish_exec("echo 'src/*.rs'");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "src/*.rs");
}

#[test]
fn test_dollar_at_in_script() {
    let tmp = helpers::TempDir::new();
    let script = tmp.write_file("test.sh", "echo \"$@\"\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kish"))
        .args([script.to_str().unwrap(), "a", "b", "c"])
        .output().expect("failed");
    // "$@" should preserve each param, echo joins with spaces
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a b c") || stdout.contains("a") && stdout.contains("b") && stdout.contains("c"));
}

#[test]
fn test_tilde_expansion() {
    let out = kish_exec("echo ~");
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(stdout.starts_with('/'), "tilde should expand to home dir, got: {}", stdout);
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/exec/ tests/parser_integration.rs
git commit -m "feat(phase3): wire full expansion into executor, add integration tests"
```

---

## Subsequent Phases

This plan covers **Phase 3 only** (full word expansion). After this phase:

- **Phase 4:** Full redirection + here-document I/O (expand here-doc bodies, read-write fd handling)
- **Phase 5:** Control structure execution (if, for, while, until, case, functions)
- **Phase 6:** Special builtins (set, export, trap, eval, exec) + alias expansion
- **Phase 7:** Signals and errexit
- **Phase 8:** Subshell environment isolation
