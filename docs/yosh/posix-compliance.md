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

- **`LC_COLLATE`**: pattern range `[a-z]` uses Unicode codepoint
  ordering; `test` string compare uses bytewise comparison on the
  UTF-8 encoding (`str::cmp`), which equals C-locale ordering on
  ASCII strings. Outside ASCII the two orderings agree only
  coarsely (UTF-8 leading-byte order tracks codepoint order), so
  neither is a full C-locale collation for non-ASCII input.
- **`LC_CTYPE`**: POSIX character classes (`[[:alpha:]]`,
  `[[:digit:]]`, `[[:upper:]]`, `[[:lower:]]`, `[[:alnum:]]`,
  `[[:xdigit:]]`, `[[:space:]]`, `[[:blank:]]`, `[[:cntrl:]]`,
  `[[:print:]]`, `[[:graph:]]`, `[[:punct:]]`) match ASCII per
  C-locale definitions.
- **`LC_MESSAGES`**: yosh diagnostics are emitted in English. The
  variable is preserved for child processes.
- **`LC_MONETARY`** / **`LC_TIME`**: variable preserved; no yosh
  builtin currently consults them.
- **`LC_NUMERIC`**: the native `printf` builtin always uses `.` as the
  radix character (C-locale semantics, consistent with the rest of
  yosh's internal locale handling); `LC_NUMERIC` affects only child
  processes such as `/usr/bin/printf`. yosh exports `LC_NUMERIC`
  unchanged.
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

## Invocation: login shells (`-l`)

POSIX XCU `sh` defines no login shell and no `-l` option. yosh
implements `-l` as an extension, following dash:

- `yosh -l`, or an argv[0] that starts with `-` (as `login(1)` and
  terminal applications exec a login shell), makes the shell a login
  shell. `+l` turns it off; the last of `-l`/`+l` wins, also over
  argv[0]. `$0` keeps argv[0] unchanged (`-yosh`).
- A login shell reads `/etc/profile`, then `$HOME/.profile`, before
  any command — interactive or not (`yosh -l script.sh`, `yosh -l -c`
  and a piped stdin included). An interactive login shell reads them
  before `~/.yoshrc` and `$ENV`; those two remain interactive-only, as
  POSIX specifies for `$ENV`. Missing files are skipped silently; a
  syntax error is reported with the file name and the shell goes on;
  `exit` in a profile ends the shell there.
- `$-` reports `l` (dash does; bash does not). It is invocation
  state, like `c` and `s`: `set -l` and `set -o login` are errors.
- Not implemented: bash's `--login` spelling, bash's
  `~/.bash_profile` / `~/.bash_login` search, and a logout file read
  when a login shell exits.

Design record: `docs/superpowers/specs/2026-11-04-login-shell-design.md`.
