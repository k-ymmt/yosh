# E2E XFAIL SP3 — `read` Builtin Implementation

**Date:** 2026-05-14
**Status:** Design (no implementation in this spec)
**Type:** New builtin + XFAIL cleanup
**Roadmap parent:** [`2026-05-13-e2e-xfail-roadmap-design.md`](2026-05-13-e2e-xfail-roadmap-design.md) §SP3

## 1. Background

The roadmap partitions 55 XFAIL tests into seven sub-projects. SP1 (11
tests) and SP2 (5 tests) are complete; 39 XFails remain. SP3 closes
nine of them by implementing the POSIX-required `read` builtin
natively.

Today `read` falls through to `/usr/bin/read` (a thin macOS wrapper on
the host). The external process can read fd 0 but cannot assign back
to the parent shell's variables, so every `read VAR; echo "$VAR"`
pattern silently produces an empty variable. The fallthrough also
prevents two `exec` tests from passing: `exec_close_fd.sh` and
`exec_redir_input.sh` both verify `exec`'s redirection behaviour by
running `read` immediately afterward.

The persistent-redirect path for `exec` with no command word
(`src/exec/simple.rs:316-329`) is **already implemented** and uses
`RedirectState::apply(.., false)` to skip the restore step. SP3 does
not need to touch that path — only the native `read` builtin is
required to unblock both `exec` tests.

## 2. Scope

In scope (this spec drives one implementation plan):

- New native `read` builtin at `src/builtin/read.rs` covering POSIX
  XCU §1.4 minimum (`-r`, multi-variable IFS field split,
  last-variable-gets-remainder, backslash line continuation,
  partial-line / EOF nonzero exit, `--` option terminator, identifier
  validation).
- Wiring in `src/builtin/mod.rs` (`classify_builtin` + dispatch).
- Removal of the `# XFAIL: …` line from each of the nine target test
  files; verification under `./e2e/run_tests.sh`.
- TODO.md / memory updates on completion (per roadmap §5).

Out of scope:

- Bash extensions: `-d delim`, `-n count`, `-N count`, `-p prompt`,
  `-s` (silent), `-t timeout`, `-u fd`. Reconsider only when a real
  consumer asks.
- Interactive trap interaction for `read` (SIGINT mid-call) — POSIX
  permits interruption but interactive trap plumbing is out of SP3.
- `exec` semantics changes — the persistent-redirect path already
  works.
- Performance work for very long lines — 1-byte reads are the POSIX
  contract; revisit only with measurements.

## 3. Goals & Non-Goals

### Goals

- All 9 SP3 XFAIL tests PASS under `./e2e/run_tests.sh` after the
  `# XFAIL:` line is removed.
- `cargo test` stays green; new unit tests in `src/builtin/read.rs`
  cover argument validation, IFS field split, and backslash handling.
- Total E2E XFail count drops from 39 to 30 with zero new FAIL.
- `read_eof_returns_nonzero.sh` (already PASS today via fallthrough)
  continues to PASS with the native builtin.

### Non-Goals

- Bash-extension flags. YAGNI; nothing in the SP3 test set or
  follow-up sub-projects requires them.
- Sharing infrastructure with `getopts` (SP4). `getopts` does not
  read stdin; the imagined `LineReader` abstraction would have no
  second consumer.

## 4. Target Tests

All paths relative to repo root.

**`read`-focused (7):**

- `e2e/posix_spec/4_required_builtin/read_basic.sh`
- `e2e/posix_spec/4_required_builtin/read_partial_line.sh`
- `e2e/posix_spec/4_required_builtin/read_multiple_vars.sh`
- `e2e/posix_spec/4_required_builtin/read_no_args.sh`
- `e2e/posix_spec/4_required_builtin/read_last_var_gets_remainder.sh`
- `e2e/posix_spec/4_required_builtin/read_r_preserves_backslash.sh`
- `e2e/posix_spec/4_required_builtin/read_strips_ifs.sh`

**`exec`-with-`read` (2):**

- `e2e/posix_spec/4_special_builtin/exec_close_fd.sh`
- `e2e/posix_spec/4_special_builtin/exec_redir_input.sh`

**Regression watch (1, currently PASS):**

- `e2e/posix_spec/4_required_builtin/read_eof_returns_nonzero.sh`

## 5. Architecture

### 5.1 File layout

```
src/builtin/
  read.rs            (new) builtin_read + helpers + unit tests
  mod.rs             (edit) classify_builtin + dispatch table
```

This mirrors the SP2 per-builtin module pattern (`hash.rs`,
`type.rs`).

### 5.2 Classification & dispatch

Three sites in `src/builtin/mod.rs` need `"read"` wired in:

1. **`pub mod read;`** at the top, next to `pub mod hash;` /
   `pub mod r#type;` (line 1-7 block).
2. **`BUILTIN_NAMES`** (line 12-18) — append `"read"` to the
   regular-builtin half so tab-completion sees the name.
3. **`classify_builtin`** (line 37-38) — extend the Regular arm:
   ```rust
   "cd" | "command" | "echo" | "true" | "false" | "alias" | "unalias"
   | "kill" | "wait" | "fg" | "bg" | "jobs" | "umask" | "test" | "["
   | "type" | "hash" | "read"                        // ← add
   => BuiltinKind::Regular,
   ```
4. **`exec_regular_builtin`** dispatch (line 44-76) — insert a new
   arm next to `"hash"`:
   ```rust
   "read" => read::builtin_read(args, env),
   ```

The existing test in the same file (`classify_builtin` tests at
~line 97-118) should grow a `classify_builtin("read") =>
BuiltinKind::Regular` assertion alongside the others.

### 5.3 Entry signature

```rust
pub(crate) fn builtin_read(
    args: &[String],
    env: &mut ShellEnv,
) -> Result<i32, ShellError>
```

Matches `hash::builtin_hash` and `r#type::builtin_type` exactly. Read
from `STDIN_FILENO` (fd 0) — yosh's executor has already applied any
redirections by the time `exec_regular_builtin` invokes the
function.

### 5.4 Internal structure

`src/builtin/read.rs` splits into three concerns:

1. **Argument parsing** — `fn parse_args(args: &[String]) -> Result<ParsedArgs, ArgError>` returning `{ raw: bool, var_names: Vec<String> }`. Pure, fully unit-testable.
2. **Line reader** — `fn read_logical_line(raw: bool, reader: &mut dyn ByteReader) -> io::Result<(Vec<Byte>, bool /* hit_eof */)>` where `ByteReader` is a trait with `fn read_byte(&mut self) -> io::Result<Option<u8>>`. Production wrapper calls `libc::read(STDIN_FILENO, &mut [0u8; 1], 1)`. Tests inject a `Cursor<&[u8]>`-backed reader.
3. **Field split & assign** — `fn split_and_assign(line: Vec<Byte>, vars: &[String], env: &mut ShellEnv) -> Result<(), ShellError>`. Reads `IFS` from `env`, splits per §6, calls `env.assign_var(name, value)` for each.

`Byte` is `struct Byte { value: u8, escaped: bool }` so split can
ignore IFS bytes that came from `\X` in non-`-r` input.

## 6. Field splitting & IFS

POSIX §2.6.5 split rules, scoped to `read` (separate from the
expander's `field_split` module — that one operates on already-split
words from parameter expansion).

### 6.1 IFS classification

- `ifs_raw = env.vars.get("IFS").unwrap_or(" \t\n")`. If unset, use
  `" \t\n"`. If set to empty string, no splitting occurs — the entire
  line goes into the first var (or N=1 var).
- Split `ifs_raw` bytes into:
  - **`ws_ifs`**: subset of `{ b' ', b'\t', b'\n' }` present in
    `ifs_raw` (POSIX whitespace IFS).
  - **`sep_ifs`**: all other bytes in `ifs_raw`.

### 6.2 Algorithm

Input: `line: &[Byte]`, `vars: &[String]` of length N ≥ 1.

1. **Trim leading `ws_ifs`** from `line`. (Each byte whose `value`
   is in `ws_ifs` AND `escaped == false`.)
2. If N == 1:
   - Trim trailing `ws_ifs` from the remaining `line`.
   - Assign the literal byte stream (escape flag dropped) to `vars[0]`.
   - Return.
3. For `i` in `0..N-1`:
   - Walk `line` forward to find the next IFS byte. Collect everything
     before it as `field_i`.
   - The terminator is either: one `sep_ifs` byte, or one or more
     consecutive `ws_ifs` bytes. Consume the terminator.
   - After the terminator, **greedily consume any additional ws_ifs**
     run (so `a   b   c` collapses correctly).
   - Assign `field_i` to `vars[i]`.
   - If `line` runs out before all N-1 fields are filled, the
     remaining vars get the empty string. (POSIX allows this; not
     covered by any SP3 test but specified for completeness.)
4. The remainder of `line` is assigned to `vars[N-1]` after **only
   trailing `ws_ifs` is trimmed**. Mid-string IFS bytes are preserved.

### 6.3 Worked examples

- `read_strips_ifs.sh`: `IFS=" \t\n"`, input `   hello   `, vars `[line]`.
  Step 1 → `hello   `. N=1, step 2 trims trailing → `hello`. ✓
- `read_multiple_vars.sh`: input `a b c`, vars `[x, y, z]`.
  Step 1 → `a b c`. Iter 1: `x=a`, consume ` `. Iter 2: `y=b`,
  consume ` `. Step 4: `z=c`. ✓
- `read_last_var_gets_remainder.sh`: input `a b c d`, vars `[x, y]`.
  Step 1 → `a b c d`. Iter 1: `x=a`, consume ` `. Step 4: trim
  trailing ws → `y=b c d`. ✓
- `read_r_preserves_backslash.sh`: `-r`, input `a\b\n`. Reader
  produces `[a, \, b]` (all unescaped). N=1, no ws to trim,
  `line=a\b`. ✓

## 7. Reading the logical line

### 7.1 EOF semantics

- `libc::read(0, buf, 1)` returning 0 with **no bytes already
  accumulated** → the `read` call as a whole hits EOF. `vars[0..N]`
  are assigned the empty string (POSIX). Builtin exit code 1, no
  stderr.
- `libc::read(0, buf, 1)` returning 0 with **partial bytes in the
  buffer** → "partial line". Run the normal split/assign over what we
  have. Builtin exit code 1, no stderr.
- Newline reached → normal completion, exit 0.

### 7.2 Backslash handling (no `-r`)

State machine per byte:

```
Normal     -- '\' -->  Escape
Normal     -- '\n' -->  EOL (consume, don't store)
Normal     -- other -->  push Byte { value, escaped: false }
Escape     -- '\n' -->  Normal (line continuation; consume both, store nothing)
Escape     -- other -->  push Byte { value, escaped: true }, Normal
```

In `-r` mode, the Escape state is skipped — `\` is just a byte.

### 7.3 EINTR

Wrap `libc::read` in a loop that retries on `EINTR`. SIGINT-driven
abort of `read` requires interactive trap plumbing that is out of
scope; the EINTR retry preserves correctness in the absence of trap
handlers.

## 8. Error handling

### 8.1 Argument errors

Error messages follow the existing special-builtin format
(`yosh: <name>: \`...': ...` with backticks around offending tokens):

| Case | stderr | Exit |
|------|--------|------|
| No variable name | `yosh: read: missing variable name` | 1 |
| Unknown flag (e.g. `-x`) | `yosh: read: -x: invalid option` | 1 |
| Invalid identifier (e.g. `1foo`) | `` yosh: read: `1foo': not a valid identifier `` | 1 |
| Readonly target var | `` yosh: read: `NAME': readonly variable `` | 1 |

Identifier validation reuses `crate::parser::word::is_valid_name`,
already used by `builtin_export`/`builtin_unset`/`builtin_readonly`
in `src/builtin/special.rs`. No hoist required.

### 8.2 I/O errors

`libc::read` returning -1 with errno != EINTR → `yosh: read: <strerror>`,
exit 1. Bytes accumulated so far are **not** assigned (POSIX leaves
this implementation-defined; not assigning is safest).

## 9. Wiring through the executor

No executor changes required. `exec_regular_builtin` already
dispatches via the `BuiltinKind::Regular` arm with redirections
applied (and restored on return). `read` is read-only from fd 0; no
extra plumbing.

The `exec` no-command persistent-redirect path
(`src/exec/simple.rs:316-329`) already does the right thing for
`exec < file` — verified by reading the existing code. SP3 does not
modify it.

## 10. Testing

### 10.1 Unit tests (in `src/builtin/read.rs`)

- `parse_args_empty_errors`
- `parse_args_dash_r`
- `parse_args_double_dash_terminator`
- `parse_args_unknown_flag_errors`
- `parse_args_invalid_identifier_errors`
- `read_line_basic_terminates_at_newline`
- `read_line_partial_line_signals_eof`
- `read_line_eof_with_no_bytes`
- `read_line_backslash_newline_continues`
- `read_line_backslash_other_keeps_literal`
- `read_line_r_preserves_backslash`
- `read_line_eintr_retries`
- `split_n_eq_1_trims_both_sides`
- `split_n_gt_1_first_fields_then_remainder`
- `split_remainder_keeps_internal_ifs`
- `split_empty_ifs_no_split`
- `split_sep_ifs_treated_as_single_separator`
- `split_escaped_byte_not_treated_as_ifs`

### 10.2 E2E (XFAIL cleanup)

For each of the 9 SP3 test files: delete the `# XFAIL: …` header
line and verify `./e2e/run_tests.sh --filter=<name>` reports PASS.
End-of-SP run of the full suite must show XFail count = 30 (from 39)
with no new FAIL or TIMEDOUT.

### 10.3 Manual smoke

```sh
cargo build
./target/debug/yosh -c 'echo hello | { read line; echo "[$line]"; }'           # [hello]
./target/debug/yosh -c 'echo a b c   | { read x y; echo "x=[$x] y=[$y]"; }'    # x=[a] y=[b c]
./target/debug/yosh -c 'printf "partial" | { read line; echo "[$line]"; echo $?; }'  # [partial], 1
./target/debug/yosh -c 'IFS=: ; echo a:b:c | { read x y; echo "x=[$x] y=[$y]"; }'    # x=[a] y=[b:c]
./target/debug/yosh -c 'printf "a\\\\b\n" | { read line; echo "[$line]"; }'     # [ab]   (backslash escape consumed)
./target/debug/yosh -c 'printf "a\\\\b\n" | { read -r line; echo "[$line]"; }'  # [a\b]
```

## 11. Open Questions (resolve during implementation)

### 11.1 `exec_close_fd.sh` expected exit

The current test expects `EXPECT_EXIT: 0`:

```sh
exec 3>&-
read line 0<&3 2>/dev/null
```

`exec 3>&-` is a no-op on fd 3 (which was never open). The subsequent
`read line 0<&3` redirects fd 0 from closed fd 3 — POSIX makes this a
redirection error in a non-special builtin → exit nonzero. dash
reports exit 1 for this pattern in interactive tests; bash likewise.

**Plan:** during implementation, run the exact script against
bash/dash and confirm. If both report exit 1, fix the test header to
`EXPECT_EXIT: 1` in the same commit and call it out in the commit
message (per roadmap §5.3). If POSIX-conforming shells diverge,
record the divergence in TODO.md and pick the dash semantics.

### 11.2 `IFS=""` semantics

POSIX: if `IFS` is set but empty, no splitting. The spec specifies
this above; verify the test suite never exercises it (it does not
today) so we can land the implementation without an explicit
end-to-end test.

### 11.3 (resolved during spec writing — kept for traceability)

The shared identifier validator already exists at
`crate::parser::word::is_valid_name`. No hoist or refactor is needed;
`src/builtin/read.rs` reuses it directly. This section originally
flagged a possible refactor; verified during spec self-review that it
is not required.

## 12. Acceptance Criterion

SP3 closes when **all of the following** are true:

1. The 9 SP3 test files no longer carry `# XFAIL:` and every one
   reports PASS under `./e2e/run_tests.sh`.
2. `./e2e/run_tests.sh` end-of-suite summary shows XFail = 30 (down
   from 39) with no new FAIL / TIMEDOUT.
3. `cargo test` (workspace) passes.
4. `cargo build` (workspace) passes.
5. `TODO.md` reflects closure: SP3 row removed from the roadmap
   checklist; `Future: POSIX Required Builtin Implementation` entry
   for `read [-r] var...` removed; any SP3 follow-up items recorded
   under a new `### SP3 follow-ups (non-blocking)` heading.
6. Memory: `project_e2e_xfail_roadmap.md` updated to mark SP3 COMPLETE
   with date and follow-up status.

## 13. Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 1-byte stdin reads slow on huge inputs | Low | Low (POSIX contract) | Document; revisit if benchmarks show pressure |
| `EINTR` retry loops forever under signal storms | Very low | Medium | Same loop pattern as existing builtins; SIGINT plumbing absent today |
| Native `read` reveals a hidden assumption in interactive line editor (e.g. `read` invoked from PS prompt) | Low | Low | Manual smoke covers piped + here-string; no interactive `read` test exists |
| `exec_close_fd.sh` POSIX answer differs from dash | Low | Low | Open question 11.1; fix test header in same commit |

## 14. Out-of-Scope Follow-ups (file under TODO.md after SP3 lands)

- Bash extensions (`-d`, `-n`, `-N`, `-p`, `-s`, `-t`, `-u`) — defer
  until a concrete consumer surfaces.
- `read` SIGINT trap interaction — wait for SP6's PTY work to land
  before re-evaluating.
- (Originally listed a `is_valid_identifier` hoist follow-up. Removed
  during spec self-review: the validator is already shared via
  `crate::parser::word::is_valid_name`.)
