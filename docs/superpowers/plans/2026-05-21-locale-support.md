# yosh Locale Support (POSIX Conformance Closure) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement POSIX §8.2 locale resolution (`LC_*`/`LANG` priority order), add POSIX character classes (`[[:alpha:]]` et al.) to pattern matching, document the LC_NUMERIC pass-through stance, repair the broken `LANG_default_collate.sh` E2E, and close the `## Future: POSIX Conformance Bugs` section of `TODO.md`.

**Architecture:** Approach A from the design spec — yosh fixes on C/POSIX locale semantics internally; non-C locale values are preserved in `ShellEnv.vars` and exported to child processes unchanged, but yosh's internal pattern matching and `test` string comparison interpret them as C. No `libc::setlocale` calls; no new crate dependencies; no `unsafe`. POSIX character classes are added to `src/expand/pattern.rs` as a new `BracketItem::Class` variant with C-locale ASCII definitions.

**Tech Stack:** Rust (workspace at `/Users/kazukiyamamoto/Projects/rust/kish`), `cargo test` for unit tests, `./e2e/run_tests.sh` for POSIX E2E.

**Design spec:** `docs/superpowers/specs/2026-05-21-locale-support-design.md`

---

## File Structure

| Path | Action | Responsibility |
|---|---|---|
| `src/env/locale.rs` | Create | `LocaleCategory`, `LocaleSource`, `ResolvedLocale`, `resolve()`, `is_c_locale()` |
| `src/env/mod.rs` | Modify | Add `pub mod locale;` |
| `src/expand/pattern.rs` | Modify | Add `PosixClass` enum, `BracketItem::Class` variant, `try_parse_posix_class`, doc comment for LC_COLLATE=C semantics |
| `src/builtin/test.rs` | Modify | Doc comment only on string compare ops |
| `e2e/posix_spec/8_env_vars/LANG_default_collate.sh` | Modify | Repair broken `echo b a` → `printf '%s\n' b a`; drop XFAIL directive |
| `e2e/posix_spec/8_env_vars/LC_ALL_overrides_LC_COLLATE.sh` | Create | New E2E |
| `e2e/posix_spec/8_env_vars/LANG_used_when_LC_unset.sh` | Create | New E2E |
| `e2e/posix_spec/8_env_vars/LC_NUMERIC_passthrough.sh` | Create | New E2E |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_alpha_in_case.sh` | Create | New E2E |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_digit_in_case.sh` | Create | New E2E |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_negate.sh` | Create | New E2E |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_mixed.sh` | Create | New E2E |
| `docs/yosh/posix-compliance.md` | Create | yosh locale-compliance posture |
| `TODO.md` | Modify | Delete `## Future: POSIX Conformance Bugs` section |
| `~/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md` | Modify | Record locale closure |

---

## Task 1: Locale Resolution API (`src/env/locale.rs`)

**Files:**
- Create: `src/env/locale.rs`
- Modify: `src/env/mod.rs:1-7` (module declarations)

- [ ] **Step 1: Create `src/env/locale.rs` with stub definitions so tests can compile**

```rust
//! POSIX §8.2 locale resolution.
//!
//! yosh fixes on C/POSIX locale semantics internally; non-C locale
//! values are preserved as variables and exported to child processes
//! unchanged, but yosh's internal pattern matching and test
//! comparisons interpret them as C. See
//! `docs/yosh/posix-compliance.md`.

use crate::env::ShellEnv;

/// POSIX locale categories.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LocaleCategory {
    Collate,
    Ctype,
    Messages,
    Monetary,
    Numeric,
    Time,
}

impl LocaleCategory {
    fn env_var_name(self) -> &'static str {
        match self {
            LocaleCategory::Collate => "LC_COLLATE",
            LocaleCategory::Ctype => "LC_CTYPE",
            LocaleCategory::Messages => "LC_MESSAGES",
            LocaleCategory::Monetary => "LC_MONETARY",
            LocaleCategory::Numeric => "LC_NUMERIC",
            LocaleCategory::Time => "LC_TIME",
        }
    }
}

/// Which variable produced the resolved value.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LocaleSource {
    LcAll,
    LcCategory,
    Lang,
    Default,
}

/// Resolved locale for a single category.
#[derive(Clone, Debug)]
pub struct ResolvedLocale {
    pub category: LocaleCategory,
    pub value: String,
    pub source: LocaleSource,
}

/// Resolve `category` per POSIX §8.2:
/// `LC_ALL` > `LC_<category>` > `LANG` > `"C"`.
///
/// Empty-string values are treated as "unset" per POSIX §8.2.
pub fn resolve(_env: &ShellEnv, _category: LocaleCategory) -> ResolvedLocale {
    unimplemented!()
}

/// True iff `value` names the POSIX (C-equivalent) locale.
///
/// POSIX XBD §7.2 specifies `"C"` and `"POSIX"` as the portable
/// locale names that produce identical behaviour. Empty string is
/// treated as "unset" by [`resolve`] and therefore never reaches
/// this predicate in normal use, but is accepted as `true` for
/// safety.
pub fn is_c_locale(_value: &str) -> bool {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn empty_env() -> ShellEnv {
        ShellEnv::new("yosh", vec![])
    }

    #[test]
    fn default_when_all_unset() {
        let env = empty_env();
        let r = resolve(&env, LocaleCategory::Collate);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::Default);
        assert_eq!(r.category, LocaleCategory::Collate);
    }

    #[test]
    fn lang_used_when_lc_unset() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        let r = resolve(&env, LocaleCategory::Ctype);
        assert_eq!(r.value, "en_US.UTF-8");
        assert_eq!(r.source, LocaleSource::Lang);
    }

    #[test]
    fn lc_category_overrides_lang() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        env.assign_var("LC_COLLATE", "C").unwrap();
        let r = resolve(&env, LocaleCategory::Collate);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::LcCategory);
    }

    #[test]
    fn lc_all_overrides_lc_category_and_lang() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        env.assign_var("LC_COLLATE", "fr_FR.UTF-8").unwrap();
        env.assign_var("LC_ALL", "C").unwrap();
        let r = resolve(&env, LocaleCategory::Collate);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::LcAll);
    }

    #[test]
    fn empty_lc_all_is_unset() {
        let mut env = empty_env();
        env.assign_var("LC_ALL", "").unwrap();
        env.assign_var("LC_COLLATE", "C").unwrap();
        let r = resolve(&env, LocaleCategory::Collate);
        // Empty LC_ALL must fall through to LC_COLLATE.
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::LcCategory);
    }

    #[test]
    fn empty_lc_category_falls_through_to_lang() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        env.assign_var("LC_NUMERIC", "").unwrap();
        let r = resolve(&env, LocaleCategory::Numeric);
        assert_eq!(r.value, "en_US.UTF-8");
        assert_eq!(r.source, LocaleSource::Lang);
    }

    #[test]
    fn empty_lang_falls_through_to_default() {
        let mut env = empty_env();
        env.assign_var("LANG", "").unwrap();
        let r = resolve(&env, LocaleCategory::Messages);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::Default);
    }

    #[test]
    fn each_category_reads_its_own_var() {
        let mut env = empty_env();
        env.assign_var("LC_COLLATE", "v_collate").unwrap();
        env.assign_var("LC_CTYPE", "v_ctype").unwrap();
        env.assign_var("LC_MESSAGES", "v_msg").unwrap();
        env.assign_var("LC_MONETARY", "v_mon").unwrap();
        env.assign_var("LC_NUMERIC", "v_num").unwrap();
        env.assign_var("LC_TIME", "v_time").unwrap();
        assert_eq!(resolve(&env, LocaleCategory::Collate).value, "v_collate");
        assert_eq!(resolve(&env, LocaleCategory::Ctype).value, "v_ctype");
        assert_eq!(resolve(&env, LocaleCategory::Messages).value, "v_msg");
        assert_eq!(resolve(&env, LocaleCategory::Monetary).value, "v_mon");
        assert_eq!(resolve(&env, LocaleCategory::Numeric).value, "v_num");
        assert_eq!(resolve(&env, LocaleCategory::Time).value, "v_time");
    }

    #[test]
    fn is_c_locale_recognizes_portable_names() {
        assert!(is_c_locale("C"));
        assert!(is_c_locale("POSIX"));
        assert!(is_c_locale(""));
    }

    #[test]
    fn is_c_locale_rejects_others() {
        assert!(!is_c_locale("en_US.UTF-8"));
        assert!(!is_c_locale("ja_JP.UTF-8"));
        assert!(!is_c_locale("c"));   // case-sensitive per POSIX
    }
}
```

- [ ] **Step 2: Register the module in `src/env/mod.rs`**

Open `src/env/mod.rs` and add `pub mod locale;` in the module-declaration block at the top (after `pub mod jobs;`, keeping alphabetical order):

```rust
pub mod aliases;
pub mod default_path;
pub mod exec_state;
pub mod jobs;
pub mod locale;      // ← new
pub mod shell_mode;
pub mod traps;
pub mod vars;
```

- [ ] **Step 3: Run tests — expect failure (panic from `unimplemented!()`)**

```bash
cargo test --lib env::locale -- --nocapture
```

Expected: tests compile, then panic at `unimplemented!()` inside `resolve` or `is_c_locale`. Confirms test wiring is correct and stubs are reachable.

- [ ] **Step 4: Implement `resolve()` and `is_c_locale()`**

Replace the two stub bodies in `src/env/locale.rs`:

```rust
pub fn resolve(env: &ShellEnv, category: LocaleCategory) -> ResolvedLocale {
    if let Some(v) = env.vars.get("LC_ALL").filter(|s| !s.is_empty()) {
        return ResolvedLocale {
            category,
            value: v.to_string(),
            source: LocaleSource::LcAll,
        };
    }
    if let Some(v) = env
        .vars
        .get(category.env_var_name())
        .filter(|s| !s.is_empty())
    {
        return ResolvedLocale {
            category,
            value: v.to_string(),
            source: LocaleSource::LcCategory,
        };
    }
    if let Some(v) = env.vars.get("LANG").filter(|s| !s.is_empty()) {
        return ResolvedLocale {
            category,
            value: v.to_string(),
            source: LocaleSource::Lang,
        };
    }
    ResolvedLocale {
        category,
        value: "C".to_string(),
        source: LocaleSource::Default,
    }
}

pub fn is_c_locale(value: &str) -> bool {
    value.is_empty() || value == "C" || value == "POSIX"
}
```

- [ ] **Step 5: Run tests — expect PASS**

```bash
cargo test --lib env::locale
```

Expected: all 10 tests pass.

- [ ] **Step 6: Verify the whole crate still builds**

```bash
cargo build
```

Expected: success.

- [ ] **Step 7: Commit**

```bash
git add src/env/locale.rs src/env/mod.rs
git commit -m "feat(env): add POSIX locale resolution API

Implements src/env/locale.rs with LocaleCategory, LocaleSource,
ResolvedLocale, resolve(), and is_c_locale() per POSIX §8.2
priority order LC_ALL > LC_<category> > LANG > \"C\".

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 2: POSIX Character Classes (`src/expand/pattern.rs`)

**Files:**
- Modify: `src/expand/pattern.rs` (extend `BracketItem`, add `PosixClass`, extend `parse_bracket`, add tests)

- [ ] **Step 1: Write failing tests at the bottom of `src/expand/pattern.rs::tests` module**

Add the following inside the existing `mod tests` block (just before the closing `}`):

```rust
    // ── POSIX character classes ──

    #[test]
    fn class_alpha_matches_letter() {
        assert!(matches("[[:alpha:]]", "a"));
        assert!(matches("[[:alpha:]]", "Z"));
    }

    #[test]
    fn class_alpha_rejects_digit() {
        assert!(!matches("[[:alpha:]]", "5"));
        assert!(!matches("[[:alpha:]]", "_"));
    }

    #[test]
    fn class_upper_matches_only_upper() {
        assert!(matches("[[:upper:]]", "A"));
        assert!(!matches("[[:upper:]]", "a"));
    }

    #[test]
    fn class_lower_matches_only_lower() {
        assert!(matches("[[:lower:]]", "z"));
        assert!(!matches("[[:lower:]]", "Z"));
    }

    #[test]
    fn class_digit() {
        assert!(matches("[[:digit:]]", "0"));
        assert!(matches("[[:digit:]]", "9"));
        assert!(!matches("[[:digit:]]", "a"));
    }

    #[test]
    fn class_alnum() {
        assert!(matches("[[:alnum:]]", "5"));
        assert!(matches("[[:alnum:]]", "a"));
        assert!(!matches("[[:alnum:]]", "_"));
    }

    #[test]
    fn class_xdigit() {
        assert!(matches("[[:xdigit:]]", "0"));
        assert!(matches("[[:xdigit:]]", "f"));
        assert!(matches("[[:xdigit:]]", "F"));
        assert!(!matches("[[:xdigit:]]", "g"));
    }

    #[test]
    fn class_space_matches_whitespace() {
        assert!(matches("[[:space:]]", " "));
        assert!(matches("[[:space:]]", "\t"));
        assert!(!matches("[[:space:]]", "a"));
    }

    #[test]
    fn class_blank_matches_horizontal_only() {
        assert!(matches("[[:blank:]]", " "));
        assert!(matches("[[:blank:]]", "\t"));
        assert!(!matches("[[:blank:]]", "\n"));
    }

    #[test]
    fn class_cntrl_matches_control() {
        assert!(matches("[[:cntrl:]]", "\x01"));
        assert!(matches("[[:cntrl:]]", "\x7f"));
        assert!(!matches("[[:cntrl:]]", "a"));
    }

    #[test]
    fn class_print_includes_space() {
        assert!(matches("[[:print:]]", " "));
        assert!(matches("[[:print:]]", "a"));
        assert!(!matches("[[:print:]]", "\x01"));
    }

    #[test]
    fn class_graph_excludes_space() {
        assert!(matches("[[:graph:]]", "a"));
        assert!(!matches("[[:graph:]]", " "));
        assert!(!matches("[[:graph:]]", "\x01"));
    }

    #[test]
    fn class_punct_is_print_minus_alnum_space() {
        assert!(matches("[[:punct:]]", "."));
        assert!(matches("[[:punct:]]", "_"));
        assert!(!matches("[[:punct:]]", "a"));
        assert!(!matches("[[:punct:]]", "5"));
        assert!(!matches("[[:punct:]]", " "));
    }

    #[test]
    fn class_combined_with_range() {
        // [[:alpha:]0-9] matches letters OR digits
        assert!(matches("[[:alpha:]0-9]", "a"));
        assert!(matches("[[:alpha:]0-9]", "5"));
        assert!(!matches("[[:alpha:]0-9]", "_"));
    }

    #[test]
    fn class_negation_with_outer_bang() {
        // [![:digit:]] matches non-digit
        assert!(matches("[![:digit:]]", "a"));
        assert!(!matches("[![:digit:]]", "5"));
    }

    #[test]
    fn unknown_class_name_falls_through_to_literal_chars() {
        // [[:unknown:]] does not panic. The class name "unknown"
        // is not in POSIX_CLASSES, so `try_parse_posix_class`
        // returns None and the loop falls through to char-by-char
        // handling. The outer bracket then contains literals
        // `[`, `:`, `u`, `n`, `k`, `o`, `w`, `:` and is closed by
        // the second-to-last `]`. The final `]` is a trailing
        // literal char. So the pattern matches 2-char strings
        // whose first char is one of those literals and whose
        // second char is `]`.
        assert!(matches("[[:unknown:]]", "[]"));
        assert!(matches("[[:unknown:]]", "u]"));
        assert!(!matches("[[:unknown:]]", "a]"));
    }

    #[test]
    fn missing_colon_close_does_not_panic() {
        // [[:alpha] (no `:]` inside) — `try_parse_posix_class`
        // scans `alpha]` and never finds `:]`, so it returns
        // None. The outer bracket then eats `[`, `:`, `a`, `l`,
        // `p`, `h`, `a` as literals; the final `]` closes the
        // bracket. Pattern matches single chars from that set.
        assert!(matches("[[:alpha]", "a"));
        assert!(matches("[[:alpha]", "["));
        assert!(!matches("[[:alpha]", "z"));
    }
```

- [ ] **Step 2: Run tests — expect compile failure**

```bash
cargo test --lib expand::pattern -- --nocapture
```

Expected: compile passes (we are only calling existing `matches()` API), but new tests fail because POSIX classes are not yet parsed (e.g. `[[:alpha:]]` currently is treated as bracket containing `[`, `:`, `a`, `l`, `p`, `h`, `a`, `:` → never matches `"a"` alone).

- [ ] **Step 3: Add the `PosixClass` enum and the `BracketItem::Class` variant**

In `src/expand/pattern.rs`, find the `enum BracketItem` definition (around line 108) and replace with:

```rust
enum BracketItem {
    Char(char),
    Range(char, char),
    Class(PosixClass),
}

#[derive(Copy, Clone)]
enum PosixClass {
    Alpha,
    Upper,
    Lower,
    Digit,
    Alnum,
    Xdigit,
    Space,
    Blank,
    Cntrl,
    Print,
    Graph,
    Punct,
}

impl PosixClass {
    /// LC_CTYPE=C semantics: ASCII-only. Non-C locale values are
    /// currently treated as C per yosh's POSIX-compliance doc
    /// (XBD §7.2 implementation-defined).
    fn matches(self, c: char) -> bool {
        match self {
            PosixClass::Alpha => c.is_ascii_alphabetic(),
            PosixClass::Upper => c.is_ascii_uppercase(),
            PosixClass::Lower => c.is_ascii_lowercase(),
            PosixClass::Digit => c.is_ascii_digit(),
            PosixClass::Alnum => c.is_ascii_alphanumeric(),
            PosixClass::Xdigit => c.is_ascii_hexdigit(),
            PosixClass::Space => matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'),
            PosixClass::Blank => matches!(c, ' ' | '\t'),
            PosixClass::Cntrl => c.is_ascii_control(),
            PosixClass::Print => matches!(c, '\x20'..='\x7e'),
            PosixClass::Graph => matches!(c, '\x21'..='\x7e'),
            PosixClass::Punct => c.is_ascii_punctuation(),
        }
    }
}

const POSIX_CLASSES: &[(&str, PosixClass)] = &[
    ("alpha", PosixClass::Alpha),
    ("upper", PosixClass::Upper),
    ("lower", PosixClass::Lower),
    ("digit", PosixClass::Digit),
    ("alnum", PosixClass::Alnum),
    ("xdigit", PosixClass::Xdigit),
    ("space", PosixClass::Space),
    ("blank", PosixClass::Blank),
    ("cntrl", PosixClass::Cntrl),
    ("print", PosixClass::Print),
    ("graph", PosixClass::Graph),
    ("punct", PosixClass::Punct),
];

/// Try to parse a `[:class:]` POSIX character-class form.
///
/// `pat` is the slice starting AFTER the opening `[:` (i.e., the
/// first character of the class name). Returns `Some((consumed,
/// class))` where `consumed` covers the class name and the trailing
/// `:]` (so the caller advances by `2 + consumed` from the opening
/// `[`).
fn try_parse_posix_class(pat: &[char]) -> Option<(usize, PosixClass)> {
    let mut end = 0;
    while end + 1 < pat.len() {
        if pat[end] == ':' && pat[end + 1] == ']' {
            let name: String = pat[..end].iter().collect();
            for (n, c) in POSIX_CLASSES {
                if name == *n {
                    return Some((end + 2, *c));
                }
            }
            return None;
        }
        end += 1;
    }
    None
}
```

Also update `BracketItem::matches` (around line 113) to handle the new variant:

```rust
impl BracketItem {
    fn matches(&self, c: char) -> bool {
        match self {
            BracketItem::Char(x) => *x == c,
            BracketItem::Range(lo, hi) => {
                // LC_COLLATE=C semantics: byte/codepoint ordering.
                // Non-C locale values are currently treated as C
                // per yosh's POSIX-compliance doc (XBD §7.2
                // implementation-defined).
                let lo = *lo as u32;
                let hi = *hi as u32;
                let c = c as u32;
                c >= lo && c <= hi
            }
            BracketItem::Class(cls) => cls.matches(c),
        }
    }
}
```

- [ ] **Step 4: Wire `try_parse_posix_class` into `parse_bracket`**

In `src/expand/pattern.rs::parse_bracket`, modify the `while i < pat.len()` loop body (around lines 77–92). Replace the loop body with the version below, which checks for `[:class:]` before the existing range/char dispatch:

```rust
    while i < pat.len() {
        if pat[i] == ']' && !members.is_empty() {
            // Found the closing bracket
            i += 1;
            found_close = true;
            break;
        }
        // POSIX character class [:class:]
        if pat[i] == '[' && i + 1 < pat.len() && pat[i + 1] == ':' {
            if let Some((consumed, class)) = try_parse_posix_class(&pat[i + 2..]) {
                members.push(BracketItem::Class(class));
                i += 2 + consumed;
                continue;
            }
            // Fall through to literal handling on malformed class
        }
        // Range: x-y  (only if there is a '-' followed by another char before ']')
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            members.push(BracketItem::Range(pat[i], pat[i + 2]));
            i += 3;
        } else {
            members.push(BracketItem::Char(pat[i]));
            i += 1;
        }
    }
```

- [ ] **Step 5: Run tests — expect PASS**

```bash
cargo test --lib expand::pattern
```

Expected: all pattern tests pass, including the ~17 new POSIX class tests.

- [ ] **Step 6: Run full unit-test suite to check no regression**

```bash
cargo test --lib
```

Expected: all unit tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/expand/pattern.rs
git commit -m "feat(expand): support POSIX character classes [[:alpha:]] et al

Adds the 12 POSIX XBD §9.3.5 character classes (alpha, upper, lower,
digit, alnum, xdigit, space, blank, cntrl, print, graph, punct) to
bracket expressions. C-locale semantics (ASCII-only) per the
locale-support spec.

Doc-comments BracketItem::Range to record LC_COLLATE=C bytewise
ordering.

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 3: Document LC_COLLATE Bytewise Semantics on `test`

**Files:**
- Modify: `src/builtin/test.rs` (doc comment on string-compare op handler)

This task adds a single doc comment to make the C-locale stance explicit at the call site. No behaviour change.

- [ ] **Step 1: Locate the string comparison handler**

Search for the operator handlers `=` / `!=` (string equality) and any `<` / `>` (sort-order) in `src/builtin/test.rs`:

```bash
grep -n '"="\|"!="\|"<"\|">"' src/builtin/test.rs | head -20
```

Note the line numbers where string compare ops are dispatched (expected near operator-table or match arms).

- [ ] **Step 2: Add the doc comment above the string-compare dispatch**

Insert the following comment immediately above the operator-table or match block that handles string `=`, `!=` (and `<` / `>` if present):

```rust
// String comparison uses bytewise ordering (LC_COLLATE=C semantics).
// Non-C locale values are currently treated as C per yosh's
// POSIX-compliance doc (XBD §7.2 implementation-defined).
```

If `test.rs` does not implement `<` or `>` operators (POSIX `test` only mandates `=` and `!=`; sort-order ops are bash extensions), the comment still applies to `=` / `!=`.

- [ ] **Step 3: Run tests to confirm no behavioural change**

```bash
cargo test --lib builtin::test
```

Expected: all existing test pass (no logic change).

- [ ] **Step 4: Commit**

```bash
git add src/builtin/test.rs
git commit -m "docs(test): comment LC_COLLATE=C bytewise compare semantics

Records the C-locale stance at the string-compare call site so
future readers know yosh's POSIX-compliance posture without
chasing the design doc.

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 4: Repair the Broken `LANG_default_collate.sh` E2E

**Files:**
- Modify: `e2e/posix_spec/8_env_vars/LANG_default_collate.sh`

The current test reads `echo b a | sort | head -n1`. `echo b a` emits one line `b a`, so `sort` returns the same one line — the comparison `= a` always fails, regardless of locale. Replace with `printf '%s\n' b a | sort | head -n1`, which produces two lines `b`/`a` for sort.

- [ ] **Step 1: Rewrite the test file**

Overwrite `e2e/posix_spec/8_env_vars/LANG_default_collate.sh` with:

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG sets default locale category values; LANG=C → C collation
# EXPECT_EXIT: 0
LANG=C
[ "$(printf '%s\n' b a | sort | head -n1)" = a ] || exit 1
```

Note: the original `# XFAIL: deferred (TODO: locale support — tracked in TODO.md)` directive is **removed**.

- [ ] **Step 2: Restore 644 permissions per CLAUDE.md convention**

```bash
chmod 644 e2e/posix_spec/8_env_vars/LANG_default_collate.sh
```

- [ ] **Step 3: Rebuild yosh (E2E runner does NOT auto-rebuild)**

```bash
cargo build
```

Expected: success.

- [ ] **Step 4: Run this E2E to verify PASS**

```bash
./e2e/run_tests.sh --filter=LANG_default_collate
```

Expected output (key line):
```
[PASS] posix_spec/8_env_vars/LANG_default_collate.sh
```

XFail count should now be one lower than before (was 1 for this section, now 0).

- [ ] **Step 5: Commit**

```bash
git add e2e/posix_spec/8_env_vars/LANG_default_collate.sh
git commit -m "fix(e2e): repair LANG_default_collate.sh (printf instead of echo)

The original test used \`echo b a | sort | head -n1\`, which emits a
single line and therefore can never have its head match 'a'.
Replace with \`printf '%s\\n' b a | sort | head -n1\`. Drop the
XFAIL directive — the test now passes under yosh's POSIX
C-locale semantics.

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 5: New E2E Tests (Locale Resolution + POSIX Classes + LC_NUMERIC Pass-Through)

**Files:**
- Create: `e2e/posix_spec/8_env_vars/LC_ALL_overrides_LC_COLLATE.sh`
- Create: `e2e/posix_spec/8_env_vars/LANG_used_when_LC_unset.sh`
- Create: `e2e/posix_spec/8_env_vars/LC_NUMERIC_passthrough.sh`
- Create: `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_alpha_in_case.sh`
- Create: `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_digit_in_case.sh`
- Create: `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_negate.sh`
- Create: `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_mixed.sh`

- [ ] **Step 1: Create `LC_ALL_overrides_LC_COLLATE.sh`**

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_ALL
# DESCRIPTION: LC_ALL overrides LC_COLLATE; internal pattern uses C semantics
# EXPECT_EXIT: 0
LC_ALL=C
LC_COLLATE=fr_FR.UTF-8
# Under C semantics, [A-Z] matches uppercase ASCII only.
case M in [A-Z]) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 2: Create `LANG_used_when_LC_unset.sh`**

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG is used when LC_ALL and LC_<category> are unset
# EXPECT_EXIT: 0
unset LC_ALL
unset LC_COLLATE
LANG=C
case M in [A-Z]) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 3: Create `LC_NUMERIC_passthrough.sh`**

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_NUMERIC
# DESCRIPTION: LC_NUMERIC is exported to child processes unchanged
# EXPECT_EXIT: 0
command -v /usr/bin/printf >/dev/null || exit 0
out=$(LC_NUMERIC=de_DE.UTF-8 /usr/bin/printf '%.2f' 1234.5)
case "$out" in 1234.50|1234,50) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 4: Create `posix_class_alpha_in_case.sh`**

```sh
#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [[:alpha:]] matches alphabetic in case pattern
# EXPECT_EXIT: 0
case A in [[:alpha:]]) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 5: Create `posix_class_digit_in_case.sh`**

```sh
#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [[:digit:]] matches digit in case pattern
# EXPECT_EXIT: 0
case 5 in [[:digit:]]) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 6: Create `posix_class_negate.sh`**

```sh
#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [![:digit:]] matches non-digit (negation of POSIX class)
# EXPECT_EXIT: 0
case a in [![:digit:]]) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 7: Create `posix_class_mixed.sh`**

```sh
#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: POSIX class can be combined with literal range
# EXPECT_EXIT: 0
case 5 in [[:alpha:]0-9]) exit 0 ;; *) exit 1 ;; esac
```

- [ ] **Step 8: Set permissions 644 on all seven new files**

```bash
chmod 644 \
  e2e/posix_spec/8_env_vars/LC_ALL_overrides_LC_COLLATE.sh \
  e2e/posix_spec/8_env_vars/LANG_used_when_LC_unset.sh \
  e2e/posix_spec/8_env_vars/LC_NUMERIC_passthrough.sh \
  e2e/posix_spec/2_06_06_pathname_expansion/posix_class_alpha_in_case.sh \
  e2e/posix_spec/2_06_06_pathname_expansion/posix_class_digit_in_case.sh \
  e2e/posix_spec/2_06_06_pathname_expansion/posix_class_negate.sh \
  e2e/posix_spec/2_06_06_pathname_expansion/posix_class_mixed.sh
```

- [ ] **Step 9: Run the new E2E tests filtered by section**

```bash
./e2e/run_tests.sh --filter=posix_spec/8_env_vars
./e2e/run_tests.sh --filter=posix_spec/2_06_06_pathname_expansion
```

Expected: all PASS, zero XFAIL in these sections.

- [ ] **Step 10: Run the full E2E suite to confirm overall XFail dropped**

```bash
./e2e/run_tests.sh 2>&1 | tail -3
```

Expected summary: `XFail: 1` (was 2; ulimit unknown-option remains).

- [ ] **Step 11: Commit**

```bash
git add e2e/posix_spec/8_env_vars/LC_ALL_overrides_LC_COLLATE.sh \
        e2e/posix_spec/8_env_vars/LANG_used_when_LC_unset.sh \
        e2e/posix_spec/8_env_vars/LC_NUMERIC_passthrough.sh \
        e2e/posix_spec/2_06_06_pathname_expansion/posix_class_alpha_in_case.sh \
        e2e/posix_spec/2_06_06_pathname_expansion/posix_class_digit_in_case.sh \
        e2e/posix_spec/2_06_06_pathname_expansion/posix_class_negate.sh \
        e2e/posix_spec/2_06_06_pathname_expansion/posix_class_mixed.sh
git commit -m "test(e2e): add locale resolution + POSIX class + LC_NUMERIC tests

Covers:
- LC_ALL > LC_COLLATE priority (C overrides fr_FR.UTF-8 internally)
- LANG used when LC_* unset
- LC_NUMERIC passes through to /usr/bin/printf
- POSIX character classes [[:alpha:]], [[:digit:]], negation,
  mixed with literal range, in case pattern position

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 6: yosh POSIX Compliance Documentation

**Files:**
- Create: `docs/yosh/posix-compliance.md`

- [ ] **Step 1: Create `docs/yosh/posix-compliance.md` with locale section**

```markdown
# yosh POSIX Compliance

This document records yosh's stance on POSIX-defined behaviours that
admit implementation-defined choices.

## Locale (POSIX XBD §7.2, XCU §2.6.5, §8.2)

### Resolution Order

yosh resolves locale categories per POSIX §8.2:

1. `LC_ALL` (if set and non-empty)
2. `LC_<category>` (if set and non-empty)
3. `LANG` (if set and non-empty)
4. `"C"` (default)

The resolution API is `src/env/locale.rs::resolve(env, LocaleCategory)`.

### Supported Locale Values

- **`C` / `POSIX` / unset / empty**: standard C-locale behaviour.
  Pattern matching, character classes, and `test` string comparison
  use ASCII / bytewise / C-locale rules.
- **Other values (e.g. `en_US.UTF-8`)**: the variable is preserved
  in the shell environment and exported to child processes
  unchanged, but yosh's internal pattern matching, character
  classification, and `test` string comparison still use C-locale
  semantics.

POSIX XBD §7.2 allows the locale behaviour for non-POSIX locales to
be implementation-defined. yosh defines it as: "non-C values are
preserved for child processes but interpreted as C internally."

### Per-Category Notes

- **`LC_COLLATE`**: pattern range `[a-z]` and `test` string compare
  use Unicode codepoint ordering, which coincides with C-locale
  bytewise ordering.
- **`LC_CTYPE`**: POSIX character classes (`[[:alpha:]]`,
  `[[:digit:]]`, `[[:upper:]]`, `[[:lower:]]`, `[[:alnum:]]`,
  `[[:xdigit:]]`, `[[:space:]]`, `[[:blank:]]`, `[[:cntrl:]]`,
  `[[:print:]]`, `[[:graph:]]`, `[[:punct:]]`) match ASCII per
  C-locale definitions.
- **`LC_MESSAGES`**: yosh diagnostics are emitted in English. The
  variable is preserved for child processes.
- **`LC_MONETARY`** / **`LC_TIME`**: variable preserved; no yosh
  builtin currently consults them.
- **`LC_NUMERIC`**: yosh has no native `printf` builtin, so the
  variable affects only child processes (e.g., `/usr/bin/printf`).
  yosh exports `LC_NUMERIC` unchanged.
- **`NLSPATH`**: yosh does not call `catopen(3)` or `catgets(3)`;
  the variable is preserved for child processes.

### What yosh Does NOT Do

- yosh does not call `setlocale(3)`. The yosh process always runs at
  Rust's default `"C"` locale.
- yosh does not link to ICU or any other locale data library.
- yosh does not honour `LC_*` for collation order of pattern ranges
  beyond C-locale bytewise comparison.

### Future Work

- Adding `[.x.]` collating elements and `[=x=]` equivalence classes
  to bracket expressions.
- Honouring non-C `LC_COLLATE` for actual collation, via `libc`
  `strcoll_l` or a pure-Rust collator.
- LC_MESSAGES translation infrastructure.
```

- [ ] **Step 2: Commit**

```bash
git add docs/yosh/posix-compliance.md
git commit -m "docs(posix): document yosh locale compliance scope

Adds docs/yosh/posix-compliance.md recording yosh's locale-support
posture: C/POSIX strict, non-C values preserved for child
processes only, no setlocale, no ICU, no LC_MESSAGES translation.
Per-category notes for all eight LC_* / LANG / NLSPATH.

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 7: Close TODO Section and Update Memory

**Files:**
- Modify: `TODO.md` (delete `## Future: POSIX Conformance Bugs` section in its entirety)
- Modify: `~/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md`

- [ ] **Step 1: Delete the `## Future: POSIX Conformance Bugs` section from `TODO.md`**

Locate the section in `TODO.md` (currently around line 395). It contains an intro paragraph and one bullet (the locale entry). Delete:
1. The `## Future: POSIX Conformance Bugs` heading
2. The four-line intro paragraph that ends with "Each remaining entry points to the XFAIL test that documents the expected POSIX behavior."
3. The `- [ ] Locale support not implemented — ...` bullet through the entire `XFAIL test:` paragraph
4. The blank line separating it from the next section

The previous and next sections (`## Future: POSIX Required Builtin Implementation` above, `## Future: Release Skill Enhancements` below) should now be separated only by a single blank line.

- [ ] **Step 2: Verify TODO.md is otherwise unchanged**

```bash
git diff TODO.md
```

Expected: a single contiguous deletion covering the section. No additions.

- [ ] **Step 3: Update memory file `project_e2e_xfail_roadmap.md`**

Open `/Users/kazukiyamamoto/.claude/projects/-Users-kazukiyamamoto-Projects-rust-kish/memory/project_e2e_xfail_roadmap.md` and update the `description:` field and the status paragraph:

Replace the `description:` line:
```yaml
description: "55-XFAIL-test decomposition roadmap; SP1-SP7 complete (2026-05-17), trap-reset deferral closed (2026-05-19); 2 deferrals remain (ulimit, locale)"
```
with:
```yaml
description: "55-XFAIL-test decomposition roadmap; SP1-SP7 complete (2026-05-17), trap-reset closed (2026-05-19), locale closed (2026-05-21); 1 deferral remains (ulimit)"
```

Replace the status paragraph beginning "After SP1+SP2+..." with:

```markdown
After SP1+SP2+SP3+SP4+SP5+SP6+SP7: 55 - 11 - 5 - 9 - 9 - 8 - 10 - 3 = 0 unaccounted XFails. The trap-reset deferral closed 2026-05-19 (commits `9ec4799`/`ead5b26`/`8422a5e`/`f703a26`; spec `docs/superpowers/specs/2026-05-19-subshell-trap-reset-design.md`). The locale deferral closed 2026-05-21 (spec `docs/superpowers/specs/2026-05-21-locale-support-design.md`, plan `docs/superpowers/plans/2026-05-21-locale-support.md`); 1 deferral remains (ulimit unknown-option). Matches `./e2e/run_tests.sh` output `XFail: 1 Migrated: 10`.
```

- [ ] **Step 4: Run full E2E one more time to confirm acceptance**

```bash
cargo build && ./e2e/run_tests.sh 2>&1 | tail -3
```

Expected summary line: `XFail: 1 Migrated: 10` (or whatever the migrated count was previously; the key is `XFail: 1`).

- [ ] **Step 5: Run the full unit test suite as the final check**

```bash
cargo test --lib
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add TODO.md
git commit -m "chore(todo): close Future: POSIX Conformance Bugs section

All entries in the section are now resolved:
- SP1-SP5 (2026-05-13..16): 42 XFAIL tests closed
- SP6 (2026-05-16): 10 tests migrated to PTY
- SP7 (2026-05-17): documentation-only deferral marking
- 2026-05-19: trap reset in subshell closed
- 2026-05-21: locale support closed (this commit chain)

Only ulimit unknown-option deferral remains in
\"Future: POSIX Required Builtin Implementation\".

Task: TODO.md \"Future: POSIX Conformance Bugs\" closure (locale)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

The memory file is outside the project working tree and is not committed by this project's git.

---

## Acceptance Criteria (from design spec §5.4)

After Task 7 completes, verify all of the following:

1. `cargo test` passes including all new unit tests (locale resolution + POSIX classes).
2. `./e2e/run_tests.sh --filter=posix_spec/8_env_vars` and `--filter=posix_spec/2_06_06_pathname_expansion` show all PASS, zero XFAIL.
3. `./e2e/run_tests.sh` full-suite summary shows `XFail: 1` (was 2; locale resolved, ulimit unknown-option remains).
4. `TODO.md` no longer contains `## Future: POSIX Conformance Bugs`.
5. Memory `project_e2e_xfail_roadmap.md` reflects locale closure on 2026-05-21.
6. `docs/yosh/posix-compliance.md` exists and records yosh's locale-compliance posture.
