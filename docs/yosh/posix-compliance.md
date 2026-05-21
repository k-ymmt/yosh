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
