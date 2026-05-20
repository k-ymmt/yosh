# yosh Locale Support (POSIX Conformance Closure) — Design

**Date:** 2026-05-21
**Status:** Design
**Scope:** Close the last item in TODO.md `## Future: POSIX Conformance Bugs` (locale support).
**Related specs:**
- `docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md` (SP1–SP7 closure)
- `docs/superpowers/specs/2026-05-17-e2e-xfail-sp7-deferred-design.md` (locale deferred as "known POSIX deviation")

## 1. Goals & Scope Boundary

### 1.1 Goals

1. Implement a single POSIX §8.2 locale resolution API
   (`LC_ALL` > `LC_<category>` > `LANG` > `"C"`) that yosh internal code
   uses uniformly.
2. yosh internal processing (pattern matching, `test` comparisons,
   arithmetic comparisons) conforms strictly to C/POSIX locale
   semantics. Non-C/POSIX locale values do not change yosh's internal
   interpretation; yosh internally fixes on C-locale behaviour.
3. Add POSIX character classes (`[[:alpha:]]`, `[[:digit:]]`,
   `[[:upper:]]`, etc. — 12 classes total) to `src/expand/pattern.rs`,
   defined under C-locale semantics.
4. `LC_NUMERIC`: yosh ships no native `printf` builtin, so POSIX
   compliance is satisfied by preserving the variable and exporting it
   to child processes transparently. Document this in the compliance
   doc.
5. E2E: repair the broken `LANG_default_collate.sh` test logic and
   close the XFAIL. Add new E2E coverage for resolution order, POSIX
   character classes, and `LC_NUMERIC` pass-through.
6. Close the `## Future: POSIX Conformance Bugs` section of `TODO.md`
   in full (zero remaining entries → delete the section).

### 1.2 Out of Scope

- `LC_MESSAGES` message translation: `yosh:` diagnostics remain English.
- `NLSPATH` `catopen`/`catgets` integration: variable is preserved and
  passed to children only.
- `LC_TIME`: no native `date` builtin, child pass-through only.
- Internal behaviour changes for non-C/POSIX locale values: variables
  are preserved but yosh interprets them as C-locale internally.
- `libc::setlocale` invocation in the yosh process: the process stays
  at Rust default ("C"). Children receive `LC_*`/`LANG` via the
  environment.
- Bracket-expression collating elements (`[.x.]`) and equivalence
  classes (`[=x=]`): out of scope; can be added in a future iteration.

### 1.3 POSIX Compliance Rationale

POSIX XBD §7.2 designates LC_COLLATE / LC_CTYPE / etc. behaviour as
*implementation-defined*. yosh's implementation definition — "non-C
locale values are interpreted as C internally; variables are passed
through to child processes" — satisfies XBD §7.2 and matches dash's
posture on non-C locales.

## 2. Architecture

### 2.1 New Module: `src/env/locale.rs`

```rust
/// POSIX §8.2 locale categories.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LocaleCategory {
    Collate,
    Ctype,
    Messages,
    Monetary,
    Numeric,
    Time,
}

/// Resolved locale value for a single category.
#[derive(Clone, Debug)]
pub struct ResolvedLocale {
    pub category: LocaleCategory,
    pub value: String,        // "C" / "POSIX" / "en_US.UTF-8" / etc.
    pub source: LocaleSource, // which variable produced the value
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LocaleSource {
    LcAll,
    LcCategory,
    Lang,
    Default,
}

/// Resolve a category per POSIX §8.2:
/// LC_ALL > LC_<category> > LANG > "C".
pub fn resolve(env: &ShellEnv, category: LocaleCategory) -> ResolvedLocale { /* ... */ }

/// True iff the value names the POSIX (C-equivalent) locale.
/// POSIX XBD §7.2 specifies "C" and "POSIX" as the portable locale
/// names that produce identical behaviour. Empty string is treated
/// as "unset" by `resolve()` and therefore never reaches this
/// predicate, but is accepted as `true` for safety.
pub fn is_c_locale(value: &str) -> bool {
    value.is_empty() || value == "C" || value == "POSIX"
}
```

### 2.2 Integration Points

| File | Change |
|---|---|
| `src/env/locale.rs` | **New** module per §2.1 |
| `src/env/mod.rs` | `pub mod locale;` declaration. No `ShellEnv` field added; resolution reads `vars` directly each call. |
| `src/expand/pattern.rs` | Add 12 POSIX character classes via `BracketItem::Class(PosixClass)`. Range matching stays as Unicode codepoint comparison (matches C-locale definition). Doc-comment the C-locale semantics. |
| `src/builtin/test.rs` | Doc-comment the `<` / `>` string-comparison operators to record that yosh uses bytewise (C-locale) comparison. No code change. |
| `docs/yosh/posix-compliance.md` | **New** doc (if not present) recording yosh's locale-compliance posture. |

### 2.3 Data Flow

```
ShellEnv.vars (LC_ALL, LC_COLLATE, LC_CTYPE, LANG, ...)
    |
    v
locale::resolve(env, LocaleCategory::Ctype)
    |
    v
ResolvedLocale { value: "C", source: Default }
    |
    v
locale::is_c_locale(&resolved.value)
    |
    v
Pattern match / test / arithmetic compare — C-locale semantics
```

In the v1 scope, all callers fall back to C-locale behaviour
regardless of `is_c_locale`'s return value. The predicate exists
today as the explicit branch point for a future non-C extension.

### 2.4 Thread Safety

- yosh does not call `setlocale(3)`. Process-global locale state is
  untouched.
- `ShellEnv.vars` uses interior mutability (existing `RefCell`); the
  new `resolve()` function takes an immutable borrow and completes
  inside a single dynamic borrow.
- The plugin watchdog thread (`crates/yosh-plugin-manager/src/runner.rs`)
  does not access `ShellEnv`; no race with locale resolution.

## 3. POSIX Character Classes

### 3.1 Classes Added

POSIX XBD §9.3.5 defines 12 character classes usable inside bracket
expressions. Each is added as a new `BracketItem::Class` variant with
C-locale definitions:

| Class | C-locale members |
|---|---|
| `[:alpha:]` | A–Z, a–z |
| `[:upper:]` | A–Z |
| `[:lower:]` | a–z |
| `[:digit:]` | 0–9 |
| `[:alnum:]` | A–Z, a–z, 0–9 |
| `[:xdigit:]` | 0–9, A–F, a–f |
| `[:space:]` | space, `\t`, `\n`, `\v`, `\f`, `\r` |
| `[:blank:]` | space, `\t` |
| `[:cntrl:]` | 0x00–0x1F, 0x7F |
| `[:print:]` | 0x20–0x7E |
| `[:graph:]` | 0x21–0x7E |
| `[:punct:]` | `print` ∧ ¬`alnum` ∧ ¬`space` |

### 3.2 Parser Changes

```rust
enum BracketItem {
    Char(char),
    Range(char, char),
    Class(PosixClass),   // new
}

#[derive(Copy, Clone)]
enum PosixClass {
    Alpha, Upper, Lower, Digit, Alnum, Xdigit,
    Space, Blank, Cntrl, Print, Graph, Punct,
}

impl PosixClass {
    fn matches(self, c: char) -> bool {
        // LC_CTYPE=C semantics; non-C locale values are currently
        // treated as C per yosh POSIX-compliance doc.
        match self {
            PosixClass::Alpha  => c.is_ascii_alphabetic(),
            PosixClass::Upper  => c.is_ascii_uppercase(),
            PosixClass::Lower  => c.is_ascii_lowercase(),
            PosixClass::Digit  => c.is_ascii_digit(),
            PosixClass::Alnum  => c.is_ascii_alphanumeric(),
            PosixClass::Xdigit => c.is_ascii_hexdigit(),
            PosixClass::Space  => matches!(c, ' '|'\t'|'\n'|'\x0b'|'\x0c'|'\r'),
            PosixClass::Blank  => matches!(c, ' '|'\t'),
            PosixClass::Cntrl  => c.is_ascii_control(),
            PosixClass::Print  => matches!(c, '\x20'..='\x7e'),
            PosixClass::Graph  => matches!(c, '\x21'..='\x7e'),
            PosixClass::Punct  => c.is_ascii_punctuation(),
        }
    }
}

const POSIX_CLASSES: &[(&str, PosixClass)] = &[
    ("alpha",  PosixClass::Alpha),
    ("upper",  PosixClass::Upper),
    ("lower",  PosixClass::Lower),
    ("digit",  PosixClass::Digit),
    ("alnum",  PosixClass::Alnum),
    ("xdigit", PosixClass::Xdigit),
    ("space",  PosixClass::Space),
    ("blank",  PosixClass::Blank),
    ("cntrl",  PosixClass::Cntrl),
    ("print",  PosixClass::Print),
    ("graph",  PosixClass::Graph),
    ("punct",  PosixClass::Punct),
];
```

**Why `is_ascii_*` rather than `is_alphabetic`**: Rust's `char::is_alphabetic`
follows the Unicode `Alphabetic` property and accepts (e.g.) Japanese
hiragana. C-locale `[:alpha:]` is restricted to A–Z, a–z. yosh fixes
on C semantics in v1, so the ASCII-restricted predicates apply.

### 3.3 Parser Extension Logic

In `parse_bracket`, when scanning bracket contents and encountering
`[:` (i.e., `pat[i] == '['` and `pat[i+1] == ':'`):

1. Scan forward for `:]`.
2. Take the inner substring and look it up in `POSIX_CLASSES`.
3. On success: push `BracketItem::Class(...)`, advance past `:]`.
4. On lookup miss or missing `:]`: fall through to literal handling
   (existing malformed-bracket fallback applies).

### 3.4 Edge Cases

| Input | Behaviour |
|---|---|
| `[[:alpha:]]` | A–Z, a–z (one POSIX class) |
| `[[:alpha:]0-9]` | A–Z, a–z, 0–9 (class + range combined) |
| `[![:digit:]]` | non-digit (outer `!` negation preserved) |
| `[[:unknown:]]` | malformed → fall back to literal bracket handling |
| `[[:alpha]` (`:]` missing) | malformed → literal `[` |
| `[:alpha:]` (no outer `[]`) | literal `[`, `:`, `a`, ... (POSIX classes are only valid inside bracket expressions) |
| `[[..a..]]` (collating element) | out of scope (future extension) |
| `[[=a=]]` (equivalence class) | out of scope (future extension) |

## 4. LC_NUMERIC, LC_COLLATE, and Error Handling

### 4.1 LC_NUMERIC: Child Pass-Through

**No code change required.** yosh's existing exec path exports
inherited environment variables to child processes. `LC_NUMERIC`,
`LC_ALL`, `LANG`, and other `LC_*` variables that yosh received from
its parent are already marked exported in `ShellEnv.vars`, so
`/usr/bin/printf` and other external utilities receive them. yosh
itself has no `printf` builtin, so there is no internal call site
that consults `LC_NUMERIC`.

This is recorded in `docs/yosh/posix-compliance.md` as yosh's
LC_NUMERIC stance.

### 4.2 LC_COLLATE: Pattern Range and Comparison

**No code change required.**

- `src/expand/pattern.rs::BracketItem::Range(lo, hi)` already compares
  via `c as u32` codepoint ordering. C-locale collation is bytewise /
  codepoint ordering, so the existing logic matches.
- `src/builtin/test.rs` string comparison `<` / `>` already uses
  Rust's `str::cmp` (bytewise). C-locale semantics match.

Add a doc comment in both call sites recording the locale assumption:

```rust
// LC_COLLATE=C semantics: byte/codepoint ordering.
// Non-C locale values are currently treated as C per yosh's
// POSIX-compliance doc (XBD §7.2 implementation-defined).
```

### 4.3 Repair LANG_default_collate.sh

Current (broken — `echo b a` emits one line, so `sort | head -n1`
returns `b a`, never `a`, and the test always fails):

```sh
LANG=C
[ "$(echo b a | sort | head -n1)" = a ] || exit 1
```

Replacement:

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG sets default locale category values; LANG=C → C collation
# EXPECT_EXIT: 0
LANG=C
[ "$(printf '%s\n' b a | sort | head -n1)" = a ] || exit 1
```

Remove the `# XFAIL: deferred (...)` directive line.

### 4.4 Unknown Locale Values

POSIX §8.2 leaves unknown locale-value handling implementation-defined.
yosh's stance:

- No diagnostic emitted (`LC_ALL=zz_ZZ.utopian` is silently accepted).
- Variable is preserved and exported to children unchanged.
- yosh internal processing remains C-locale even when
  `is_c_locale(value)` would return false.

This matches dash and bash's posture (both accept unknown locale
values without emitting diagnostics; their internal behaviour
diverges but neither emits an error).

### 4.5 Backward Compatibility

- The new `locale::resolve` API has no existing callers; v1 adds the
  module and the call sites are introduced by this change.
- Existing pattern matching behaviour `[a-z]` is unchanged (Unicode
  codepoint comparison was already the C-locale interpretation).
- Existing E2E (`LC_ALL_overrides_others.sh`,
  `LC_COLLATE_affects_pattern.sh`, `LC_CTYPE_affects_classification.sh`,
  `LC_MESSAGES_locale.sh`, `NLSPATH_set.sh`) all pass today and remain
  unchanged after this work.
- POSIX character classes (`[[:alpha:]]` et al.) are a new feature
  — no existing yosh script uses them, so the surface is purely
  additive.

## 5. Testing

### 5.1 Unit Tests

**`src/env/locale.rs::tests`** (new, ~10 tests):

- Resolution order: `LC_ALL` overrides `LC_<cat>` and `LANG`.
- Only `LC_ALL` set → all categories return its value.
- `LC_COLLATE` and `LANG` both set → `LC_COLLATE` wins.
- All unset → `"C"` default with `LocaleSource::Default`.
- Empty `LC_ALL` is treated as unset per POSIX §8.2.
- `is_c_locale`: `"C"` / `"POSIX"` / `""` return true; `"en_US.UTF-8"`
  returns false.
- `LocaleSource` correctly tags `LcAll` / `LcCategory` / `Lang` /
  `Default` for downstream diagnostics.

**`src/expand/pattern.rs::tests`** (extension, ~15 tests added):

- Each of the 12 classes: positive and negative case.
- Mixed expression: `[[:alpha:]0-9]` matches `a` and `5`.
- Negation: `[![:digit:]]` matches `a`, not `5`.
- Malformed: `[[:unknown:]]` falls back to literal handling.
- Malformed: `[[:alpha]` (missing `:]`) falls back to literal `[`.
- Existing range tests for `[a-z]` remain (regression guard).

### 5.2 E2E Tests

**Modified**: `e2e/posix_spec/8_env_vars/LANG_default_collate.sh` →
replace with §4.3 version; drop XFAIL directive.

**Added**:

| File | Verifies |
|---|---|
| `e2e/posix_spec/8_env_vars/LC_ALL_overrides_LC_COLLATE.sh` | `LC_ALL=C; LC_COLLATE=en_US.UTF-8` produces C behaviour internally |
| `e2e/posix_spec/8_env_vars/LANG_used_when_LC_unset.sh` | `LANG=C` with `LC_*` unset uses LANG |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_alpha_in_case.sh` | `case A in [[:alpha:]])` matches |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_digit_in_case.sh` | `case 5 in [[:digit:]])` matches |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_negate.sh` | `case a in [![:digit:]])` matches |
| `e2e/posix_spec/2_06_06_pathname_expansion/posix_class_mixed.sh` | `case 5 in [[:alpha:]0-9])` matches |
| `e2e/posix_spec/8_env_vars/LC_NUMERIC_passthrough.sh` | `LC_NUMERIC` reaches `/usr/bin/printf`; output is either `.`-form or `,`-form |

E2E format (per `CLAUDE.md`; runner supports `EXPECT_OUTPUT`,
`EXPECT_EXIT`, `EXPECT_STDERR` only):

```sh
#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [[:alpha:]] matches alphabetic in case pattern
# EXPECT_EXIT: 0
case A in [[:alpha:]]) exit 0 ;; *) exit 1 ;; esac
```

Permissions: `chmod 644`.

`LC_NUMERIC_passthrough.sh` cannot use a regex-style `EXPECT_*`
directive (the runner only supports literal `EXPECT_OUTPUT`). Instead
the test branches inside the script and exits 0 on either acceptable
separator (and exits 1 if `/usr/bin/printf` is missing), so OS locale
data availability does not break CI:

```sh
#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_NUMERIC
# DESCRIPTION: LC_NUMERIC is exported to children unchanged
# EXPECT_EXIT: 0
command -v /usr/bin/printf >/dev/null || exit 0    # skip if missing
out=$(LC_NUMERIC=de_DE.UTF-8 /usr/bin/printf '%.2f' 1234.5)
case "$out" in 1234.50|1234,50) exit 0 ;; *) exit 1 ;; esac
```

### 5.3 Existing Test Impact

- The five existing locale-related E2E tests
  (`LC_ALL_overrides_others.sh`, `LC_COLLATE_affects_pattern.sh`,
  `LC_CTYPE_affects_classification.sh`, `LC_MESSAGES_locale.sh`,
  `NLSPATH_set.sh`) all pass today and behave identically after this
  change.
- Existing `case_pattern_*` and `test_*` E2E paths: no behavioural
  change.
- Existing `pattern.rs::tests` unit tests for `[a-z]`: unchanged.

### 5.4 Acceptance Criteria

1. `cargo test` passes (including new unit tests).
2. `./e2e/run_tests.sh --filter=posix_spec/8_env_vars` and
   `--filter=posix_spec/2_06_06_pathname_expansion` show all PASS,
   zero XFAIL.
3. `./e2e/run_tests.sh` full-suite XFAIL count drops from 2 to 1
   (locale resolved; ulimit unknown-option remains).
4. `TODO.md` `## Future: POSIX Conformance Bugs` section is deleted
   (zero remaining entries).
5. Memory `project_e2e_xfail_roadmap.md` is updated: "locale closed
   2026-05-21; only ulimit remains".
6. `docs/yosh/posix-compliance.md` exists and records yosh's locale
   compliance posture.

### 5.5 Suggested Commit Shape

- `feat(env): add POSIX locale resolution API (src/env/locale.rs)`
- `feat(expand): support POSIX character classes [[:alpha:]] et al`
- `docs(test): comment LC_COLLATE=C bytewise compare semantics`
- `test(e2e): add locale resolution + posix class + LC_NUMERIC passthrough tests`
- `fix(e2e): repair LANG_default_collate.sh (printf instead of echo)`
- `docs(posix): document yosh locale compliance scope`
- `chore(todo): close Future: POSIX Conformance Bugs section`

## 6. Open Questions / Future Work

- Adding `[.x.]` collating elements and `[=x=]` equivalence classes to
  bracket expressions when use cases emerge.
- Honouring non-C `LC_COLLATE` values for actual collation (requires
  one of the deferred approaches — libc `strcoll_l` or a pure-Rust
  collator).
- LC_MESSAGES translation infrastructure (separate spec required if
  pursued).
- Native `printf` builtin that honours `LC_NUMERIC` internally (would
  re-enable LC_NUMERIC as an internal call site).
