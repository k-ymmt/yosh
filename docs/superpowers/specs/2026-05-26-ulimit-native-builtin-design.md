# Native `ulimit` builtin (POSIX-minimal `-f`)

**Spec date:** 2026-05-26
**Status:** Approved (pending implementation)
**TODO origin:** "Future: POSIX Required Builtin Implementation" —
"`ulimit [-f] [num]` — resource-limit query/set. Currently uses
`/usr/bin/ulimit`. XFAIL tests: `e2e/posix_spec/4_required_builtin/ulimit_*.sh`
(1 of 3 remains XFAIL — unknown-option case)."

## 1. Problem

`ulimit` is a POSIX XCU §1.4 required builtin that yosh does not implement
natively. `classify_builtin` returns `NotBuiltin`, so the name falls through
to the external `/usr/bin/ulimit` shell wrapper (present on macOS).

The fallthrough is observably broken for the *set* case. Probed at HEAD:

```
$ yosh -c 'ulimit -f 100'; echo "exit=$?"
exit=0                       # but the limit is set in the CHILD wrapper
                             # process, NOT in yosh — a silent no-op

$ yosh -c 'ulimit -f'        # via /usr/bin/ulimit
unlimited
exit=0

$ yosh -c 'ulimit -Z'        # via /usr/bin/ulimit
/usr/bin/ulimit: line 4: ulimit: -Z: invalid option
ulimit: usage: ulimit [-SHacdfilmnpqstuvx] [limit]
exit=2
```

Because a child process cannot change the parent shell's resource limits,
`ulimit -f N` cannot do its job through the fallthrough — children of yosh do
not inherit the requested limit. A native builtin runs in the shell process
itself, so `setrlimit` actually takes effect and is inherited by subsequent
children.

Of the three e2e acceptance tests, two already pass (the no-op `set` and the
`show`, since both happen to exit 0), and one remains XFAIL:
`ulimit_unknown_option.sh` expects exit 1, but `/usr/bin/ulimit` returns exit
2.

## 2. Scope

In:
- `src/builtin/regular.rs::builtin_ulimit` — new regular builtin, `-f` only
- `src/builtin/mod.rs` — register in `BUILTIN_NAMES`, `classify_builtin`
  (`Regular`), and `exec_regular_builtin` dispatch
- Pure helpers `parse_ulimit` and `format_fsize_limit` for unit-testability
- New unit tests in `src/builtin/regular.rs` (`#[cfg(test)] mod tests`)
- Remove the `# XFAIL:` line from
  `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`
- Remove the completed `ulimit` item from `TODO.md`

Out (deferred — see §7):
- Any resource other than `-f` (`-c`, `-d`, `-n`, `-s`, `-t`, `-v`, `-a`)
- Hard/soft selectors `-H` / `-S`
- bash's 1024-byte block convention (POSIX mandates 512-byte blocks)

## 3. Design

`ulimit` is a **regular** builtin (POSIX does not classify it as special).
It needs no `ShellEnv` access — resource limits are process-global via
`libc::{getrlimit,setrlimit}` — so the signature mirrors `umask`:

```rust
pub fn builtin_ulimit(args: &[String]) -> Result<i32, ShellError>
```

### 3.1 Argument parsing (pure)

```rust
enum UlimitAction { Show, SetBlocks(u64), SetUnlimited }
enum UlimitArgError { UnknownOption(String), InvalidNumber(String), TooManyArgs }

fn parse_ulimit(args: &[String]) -> Result<UlimitAction, UlimitArgError>
```

Rules for the POSIX synopsis `ulimit [-f] [blocks]`:

| Input                       | Result                  |
|-----------------------------|-------------------------|
| `ulimit`                    | `Show`                  |
| `ulimit -f`                 | `Show`                  |
| `ulimit -f 100`             | `SetBlocks(100)`        |
| `ulimit 100`                | `SetBlocks(100)`        |
| `ulimit -f unlimited`       | `SetUnlimited`          |
| `ulimit unlimited`          | `SetUnlimited`          |
| `ulimit -Z` (other option)  | `UnknownOption("-Z")`   |
| `ulimit -f abc`             | `InvalidNumber("abc")`  |
| `ulimit -f 1 2`             | `TooManyArgs`           |

Parsing detail: option detection applies **only to the leading token**. If the
first token is `-f`, it is consumed as the (only accepted) option; any other
leading token beginning with `-` (and not exactly `-`) is `UnknownOption`. All
remaining tokens are operands — a leading `-` there is not an option, so e.g.
`ulimit -f -5` yields `InvalidNumber("-5")` (POSIX leaves negative values
unspecified; we reject). After the optional `-f`, at most one operand is
allowed: `unlimited` → `SetUnlimited`, an all-ASCII-digit string →
`SetBlocks(parse)`, otherwise `InvalidNumber`. A second operand → `TooManyArgs`.

### 3.2 Limit formatting (pure)

```rust
fn format_fsize_limit(rlim_cur: libc::rlim_t) -> String
```

`RLIM_INFINITY` → `"unlimited"`; otherwise `(rlim_cur / 512).to_string()`.
POSIX `-f` units are **512-byte blocks** (`const BLOCK_SIZE: u64 = 512`).

### 3.3 Syscall layer (thin)

- `Show`: `getrlimit(RLIMIT_FSIZE)`, print `format_fsize_limit(rlim_cur)`
  via `println!` (the **soft** limit).
- `SetBlocks(n)`: read the current `rlimit`, set both `rlim_cur` and
  `rlim_max` to `n * 512`, `setrlimit(RLIMIT_FSIZE, ...)`. Setting both
  soft and hard when no `-H`/`-S` selector is given matches bash, ksh, and
  dash; the report path shows the soft limit. (Consequence: lowering the
  hard limit is irreversible for an unprivileged process — same footgun as
  every POSIX shell.)
- `SetUnlimited`: same, with both fields set to `RLIM_INFINITY`.

`getrlimit` / `setrlimit` are called through `unsafe { ... }` exactly as
`builtin_umask` calls `libc::umask`. Platform integer-type differences for the
resource argument (`RLIMIT_FSIZE`) are handled with an explicit cast at the
call site.

### 3.4 Errors and exit codes

All `ulimit` errors exit **1**, matching the in-tree precedent set by
`builtin_read` (`UnknownFlag` → `return Ok(1)`) and the existing XFAIL test's
`EXPECT_EXIT: 1`. Messages are `yosh:`-prefixed on stderr:

| Condition                       | Message                               | Exit |
|---------------------------------|---------------------------------------|------|
| `UnknownOption("-Z")`           | `yosh: ulimit: -Z: invalid option`    | 1    |
| `InvalidNumber("abc")`          | `yosh: ulimit: abc: invalid number`   | 1    |
| `TooManyArgs`                   | `yosh: ulimit: too many arguments`    | 1    |
| `setrlimit` failure (e.g. EPERM)| `yosh: ulimit: <strerror(errno)>`     | 1    |

These are emitted directly with `eprintln!` and `return Ok(1)` (as `read`
does), rather than via `ShellError` — `RuntimeErrorKind::InvalidOption` maps to
exit 2, which would contradict the acceptance test.

## 4. Testing

### 4.1 Safety constraint

A unit test that calls `setrlimit(RLIMIT_FSIZE, small)` lowers the limit for
the **entire test binary process** (cargo runs tests as threads in one
process), which could make a concurrent test's file write fail with `SIGXFSZ`.
Therefore unit tests MUST NOT set a restrictive limit. Real `set` behavior is
verified only via e2e, where each test runs in an isolated, short-lived yosh
subprocess.

### 4.2 Unit tests (`src/builtin/regular.rs` tests)

`parse_ulimit` — one assertion per row of the §3.1 table (Show, `-f` Show,
`-f 100`, bare `100`, `-f unlimited`, bare `unlimited`, `-Z` unknown, `-f abc`
invalid, `-f 1 2` too-many).

`format_fsize_limit` — `RLIM_INFINITY` → `"unlimited"`, `51200` → `"100"`,
`0` → `"0"`.

Syscall side effects are exercised only read-only: a `getrlimit` round-trip
test may read the current limit and set it back to the same value (a no-op
that cannot break sibling tests).

### 4.3 E2E tests (`e2e/posix_spec/4_required_builtin/`)

- `ulimit_set_filesize.sh`, `ulimit_show_filesize.sh` — unchanged; still
  exit 0, now driven by the native builtin.
- `ulimit_unknown_option.sh` — delete the `# XFAIL:` line. The native builtin
  produces `yosh: ulimit: -Z: invalid option` (stderr substring `ulimit`
  matches) and exit 1, so the test flips to PASS.

## 5. Acceptance criteria

1. `cargo test` (unit) passes, including the new `parse_ulimit` /
   `format_fsize_limit` tests and `test_builtin_names_consistent_with_classify`.
2. `./e2e/run_tests.sh --filter=ulimit` reports 3 PASS, 0 XFAIL.
3. `type ulimit` reports a builtin; `command -v ulimit` resolves to the
   builtin rather than `/usr/bin/ulimit`.
4. `cargo fmt --all -- --check` and `cargo clippy --all-targets` are clean.

## 6. Files touched

- `src/builtin/regular.rs` — `builtin_ulimit`, `parse_ulimit`,
  `format_fsize_limit`, `UlimitAction`, `UlimitArgError`, unit tests
- `src/builtin/mod.rs` — `BUILTIN_NAMES`, `classify_builtin`,
  `exec_regular_builtin`
- `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh` — drop XFAIL
- `TODO.md` — remove the completed `ulimit` item

## 7. Deferred / future work

Broader resource coverage (`-c -d -n -s -t -v -a`) and hard/soft selectors
(`-H` / `-S`) are explicit non-POSIX extensions, intentionally out of scope per
the project's POSIX-first philosophy. If added later, they would extend
`UlimitAction` with a resource enum and a soft/hard selector, reusing the same
parse/format/syscall split established here. bash's 1024-byte block convention
is also deferred; POSIX mandates 512-byte blocks and the e2e tests do not assert
the numeric value, so 512 is correct and safe.
