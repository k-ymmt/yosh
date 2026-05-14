# E2E XFAIL SP4 — `getopts` Builtin Implementation

**Date:** 2026-05-14
**Roadmap:** `docs/superpowers/specs/2026-05-13-e2e-xfail-roadmap-design.md` §SP4
**Status:** Design (pre-plan)
**Type:** New required builtin

## 1. Goal & Scope

Implement POSIX XCU `getopts` as a native `yosh` regular builtin so that
the 9 XFAIL tests listed in §1.1 transition from XFAIL → PASS, and
function-scope `OPTIND` and explicit-operand iteration behave per POSIX.

After SP4 closes, the e2e suite reports `XFail: 21` (was 30 after
SP1+SP2+SP3).

### 1.1 Target XFAIL set

| Category | File |
|---|---|
| Basic option | `e2e/posix_spec/4_required_builtin/getopts_basic.sh` |
| Option with argument | `e2e/posix_spec/4_required_builtin/getopts_with_arg.sh` |
| Stacked options | `e2e/posix_spec/4_required_builtin/getopts_stacked.sh` |
| Unknown option | `e2e/posix_spec/4_required_builtin/getopts_unknown.sh` |
| Missing argument (silent) | `e2e/posix_spec/4_required_builtin/getopts_missing_arg.sh` |
| OPTIND advance | `e2e/posix_spec/4_required_builtin/getopts_optind.sh` |
| OPTIND initial value | `e2e/posix_spec/8_env_vars/OPTIND_initial_one.sh` |
| OPTIND advances | `e2e/posix_spec/8_env_vars/OPTIND_advances.sh` |
| OPTARG set | `e2e/posix_spec/8_env_vars/OPTARG_set_by_getopts.sh` |

### 1.2 In scope (POSIX conformance beyond XFAIL)

- Explicit operand iteration: `getopts spec var arg1 arg2 ...` uses the
  trailing operands instead of `"$@"`.
- Function-scope `OPTIND` save/restore on `push_scope` / `pop_scope`.

### 1.3 Out of scope (recorded in §7)

- optstring leading `-` (GNU getopt reorder extension)
- Long options (`--foo=bar`)
- `OPTERR` (bash extension)
- `set -x` integration changes
- Plugin-host exposure of getopts state

## 2. File Surface

| Path | Change |
|---|---|
| `src/builtin/getopts.rs` | **New.** Builtin entry, pure `step_getopts`, arg parser, unit tests. |
| `src/builtin/mod.rs` | Add `pub mod getopts;`, add `"getopts"` to `BUILTIN_NAMES`, add `"getopts"` arm to `classify_builtin` (Regular), add `"getopts" => getopts::builtin_getopts(args, env)` arm to `exec_regular_builtin`. |
| `src/env/vars.rs` | Extend `Scope` struct with `getopts_subindex: usize` (default 0) and `saved_optind: Option<String>` (default `None`). Update `push_scope` to snapshot the caller's `OPTIND` and reset locally to `"1"`. Update `pop_scope` to restore from `saved_optind`. |
| `src/env/mod.rs` | At `ShellEnv::new`, set `OPTIND="1"` in the global scope so `OPTIND_initial_one.sh` sees the value before `getopts` is ever invoked. |

The shape mirrors SP3 (`read.rs`): a small public entry, a parser for
the argv slice, and a pure stateless step function tested directly.

## 3. State Model

```
ShellEnv
├── vars (per-scope, walked top→bottom for reads)
│   ├── OPTIND : String      "1", "2", ...  user-visible, user-writable
│   └── OPTARG : String      argument value, or unknown-option char in silent mode
└── Scope (in VarStore)
    ├── getopts_subindex : usize           cursor within stacked argv element
    │                                       (0 = "advance to next element")
    └── saved_optind : Option<String>      pushed at function entry, popped at exit
```

### 3.1 Scope discipline

- **Shell startup:** `ShellEnv::new` sets `OPTIND="1"` in the global
  vars table. `getopts_subindex = 0` is the default field value.
- **Function entry (`push_scope`):** record the caller's current
  `OPTIND` string into the new scope's `saved_optind`, then write
  `OPTIND="1"` into the new scope (visible to the function body).
  `getopts_subindex` of the new scope starts at 0.
- **Function exit (`pop_scope`):** before dropping the scope, take its
  `saved_optind`, and write that value back to `OPTIND` so the caller
  sees its pre-call value. The popped subindex is discarded with the
  scope.
- **Manual reset by user:** when `getopts` runs and reads `OPTIND`, if
  the parsed integer is `1`, it forces `scope.getopts_subindex = 0`.
  This implements the POSIX rule that "setting OPTIND to 1 lets the
  caller start a new parse."

### 3.2 OPTIND / subindex relationship

- `OPTIND` is the **1-based index of the argv element to inspect
  next.**
- `subindex` is the **1-based offset within the current argv element**;
  `0` means "no stacking in progress, start fresh on next element."
- For `-abc`, the parser returns `a` with `OPTIND=1, subindex=2`, then
  `b` with `OPTIND=1, subindex=3`, then `c` with `OPTIND=2, subindex=0`.
- When a value is consumed from the trailing portion of an element
  (`-aval`) or from the next element (`-a val`), `OPTIND` jumps past
  that element and `subindex` resets to 0.

## 4. Parsing Algorithm

```text
builtin_getopts(args, env):
  1. parse_args:
       optstring  = args[0]
       var_name   = args[1]              # validate is_valid_name
       operands   = args[2..]            # if empty, fall back to vars.positional_params()
       silent     = optstring.starts_with(':')
       spec       = if silent { &optstring[1..] } else { &optstring }

  2. Read state:
       optind    = env.vars["OPTIND"].parse::<usize>().unwrap_or(1).max(1)
       if optind == 1 { scope.getopts_subindex = 0 }    # POSIX reset hook
       subindex  = scope.getopts_subindex

  3. step = step_getopts(spec, &operands, optind, subindex, silent)

  4. Apply step:
       env.vars[var_name] = step.var_value
       match step.optarg:
         Some(s) -> env.vars["OPTARG"] = s
         None    -> env.vars["OPTARG"] = ""        # POSIX permits unset; "" matches dash/bash
       env.vars["OPTIND"] = step.optind.to_string()
       scope.getopts_subindex = step.subindex
       if let Some(msg) = step.stderr { eprintln!("yosh: getopts: {}", msg) }
       return Ok(step.exit)
```

### 4.1 `step_getopts` (pure function)

```text
input : spec: &str, operands: &[&str], optind_in, subindex_in, silent
output: GetoptsStep { var_value, optarg, optind, subindex, exit, stderr }

  if optind_in > operands.len(): return END_OF_OPTIONS

  elt = operands[optind_in - 1]

  if subindex_in == 0:
      if elt == "--":
          # consume the "--" sentinel, advance OPTIND past it
          return { var="?", optarg=None, optind=optind_in+1, sub=0, exit=1 }
      if !elt.starts_with('-') || elt == "-":
          return END_OF_OPTIONS (with optind_in unchanged)
      cursor = 1
  else:
      cursor = subindex_in

  ch = elt.as_bytes()[cursor] as char
  cursor += 1
  rest_of_elt = cursor < elt.len()

  pos = spec.find(ch)
  if pos.is_none():
      # unknown option
      if silent:
          return { var="?", optarg=Some(ch.to_string()),
                   optind=advance_if_done(optind_in, rest_of_elt),
                   sub=if rest_of_elt { cursor } else { 0 },
                   exit=0, stderr=None }
      else:
          return { var="?", optarg=None,
                   optind=advance_if_done(optind_in, rest_of_elt),
                   sub=if rest_of_elt { cursor } else { 0 },
                   exit=0,
                   stderr=Some(format!("-{}: illegal option", ch)) }

  takes_arg = spec.as_bytes().get(pos+1) == Some(&b':')

  if !takes_arg:
      return { var=ch.to_string(), optarg=None,
               optind=advance_if_done(optind_in, rest_of_elt),
               sub=if rest_of_elt { cursor } else { 0 },
               exit=0, stderr=None }

  # takes_arg branch
  if rest_of_elt:
      # `-aval` form: rest of elt is the argument
      arg = &elt[cursor..]
      return { var=ch.to_string(), optarg=Some(arg.to_string()),
               optind=optind_in + 1, sub=0, exit=0 }

  # `-a val` form: next operand is the argument
  if optind_in + 1 > operands.len():
      # missing argument
      if silent:
          return { var=":".to_string(), optarg=Some(ch.to_string()),
                   optind=optind_in + 1, sub=0, exit=0, stderr=None }
      else:
          return { var="?".to_string(), optarg=None,
                   optind=optind_in + 1, sub=0, exit=0,
                   stderr=Some(format!(
                       "option requires an argument -- {}", ch)) }

  arg = operands[optind_in]   # 0-based; that is operand at index optind_in
  return { var=ch.to_string(), optarg=Some(arg.to_string()),
           optind=optind_in + 2, sub=0, exit=0 }

END_OF_OPTIONS:
  return { var="?", optarg=None, optind=optind_in, sub=0, exit=1 }
```

Helper:
```text
advance_if_done(optind, rest_of_elt) =
    if rest_of_elt { optind } else { optind + 1 }
```

### 4.2 Behavior table (silent vs. normal)

| Situation | Normal mode | Silent mode (`:opstring`) |
|---|---|---|
| Unknown option `-x` | `var="?"`, OPTARG `""`, stderr "−x: illegal option", exit 0 |  `var="?"`, OPTARG `"x"`, no stderr, exit 0 |
| Missing required arg | `var="?"`, OPTARG `""`, stderr "option requires an argument −− c", exit 0 | `var=":"`, OPTARG `"c"`, no stderr, exit 0 |
| End of options | `var="?"`, OPTARG `""`, exit 1 | same |
| `--` terminator | `var="?"`, OPTIND advanced past `--`, exit 1 | same |

`getopts_missing_arg.sh` uses `getopts ":a:" opt` and expects
`echo "$opt$OPTARG"` to print `:a` — silent mode produces `var=":",
OPTARG="a"`, which concatenates to `:a`. ✓

## 5. Builtin Surface (Rust)

```rust
// src/builtin/getopts.rs

pub fn builtin_getopts(args: &[String], env: &mut ShellEnv)
    -> Result<i32, ShellError>;

#[derive(Debug, PartialEq)]
enum ArgError {
    MissingOperands,           // < 2 args
    InvalidVarName(String),    // not is_valid_name
}

struct ParsedArgs<'a> {
    optstring: &'a str,
    var_name: &'a str,
    operands: Vec<&'a str>,    // explicit args[2..]; caller substitutes positional fallback
}

fn parse_args<'a>(args: &'a [String]) -> Result<ParsedArgs<'a>, ArgError>;

#[derive(Debug, PartialEq)]
struct GetoptsStep {
    var_value: String,
    optarg: Option<String>,
    optind: usize,
    subindex: usize,
    exit: i32,
    stderr: Option<String>,
}

fn step_getopts(
    spec: &str,
    operands: &[&str],
    optind_in: usize,
    subindex_in: usize,
    silent: bool,
) -> GetoptsStep;
```

### 5.1 Exit code contract

| Condition | `Ok(i32)` value |
|---|---|
| Argument parse error (missing operands, invalid var name) | `Ok(2)` (POSIX usage error) with `yosh: getopts: ...` diagnostic |
| Successful option dispatch (incl. silent/normal unknown or missing) | `Ok(0)` |
| End of options or `--` consumed | `Ok(1)` |

`exec_regular_builtin` already converts `Err(ShellError)` via the
existing match arm; `getopts` returns only `Ok(_)` and uses `eprintln!`
for diagnostics (mirroring `read.rs`).

### 5.2 Var-name validation

Use `crate::parser::word::is_valid_name` (same as `read.rs`). On
failure: stderr `yosh: getopts: `<name>': not a valid identifier`,
return `Ok(2)`.

## 6. Testing Strategy

### 6.1 Pure unit tests (`src/builtin/getopts.rs::tests`)

Each row exercises `step_getopts` directly with explicit inputs.

| # | spec | operands | optind | sub | silent | Expected output |
|---|---|---|---|---|---|---|
| 1 | `"a"` | `["-a"]` | 1 | 0 | false | var=`"a"`, optind=2, sub=0, exit=0 |
| 2 | `"a:"` | `["-aval"]` | 1 | 0 | false | var=`"a"`, OPTARG=`"val"`, optind=2, sub=0, exit=0 |
| 3 | `"a:"` | `["-a","val"]` | 1 | 0 | false | var=`"a"`, OPTARG=`"val"`, optind=3, sub=0, exit=0 |
| 4 | `"ab"` | `["-ab"]` | 1 | 0 | false | var=`"a"`, optind=1, sub=2, exit=0 |
| 5 | `"ab"` | `["-ab"]` | 1 | 2 | false | var=`"b"`, optind=2, sub=0, exit=0 |
| 6 | `"a"` | `["-x"]` | 1 | 0 | false | var=`"?"`, optind=2, sub=0, stderr Some, exit=0 |
| 7 | `"a"` | `["-x"]` | 1 | 0 | true | var=`"?"`, OPTARG=`"x"`, optind=2, sub=0, stderr None, exit=0 |
| 8 | `"a:"` | `["-a"]` | 1 | 0 | false | var=`"?"`, optind=2, sub=0, stderr Some, exit=0 |
| 9 | `"a:"` | `["-a"]` | 1 | 0 | true | var=`":"`, OPTARG=`"a"`, optind=2, sub=0, stderr None, exit=0 |
| 10 | `"a"` | `["--"]` | 1 | 0 | false | var=`"?"`, optind=2, sub=0, exit=1 |
| 11 | `"a"` | `["-a"]` | 2 | 0 | false | var=`"?"`, optind=2, sub=0, exit=1 |
| 12 | `"a"` | `["arg"]` | 1 | 0 | false | var=`"?"`, optind=1, sub=0, exit=1 |
| 13 | `"a"` | `["-"]` | 1 | 0 | false | var=`"?"`, optind=1, sub=0, exit=1 (POSIX: `-` is operand) |

### 6.2 Integration tests (`src/builtin/getopts.rs::tests`, via `make_env`)

- `OPTIND` is `"2"` after a single successful `-a` parse against `set -- -a`.
- `OPTARG` is `"value"` after `getopts "a:" opt -a value` (explicit operand path).
- positional fallback: with `set -- -a` and `getopts a opt` (no explicit operands), the result equals `getopts a opt -a`.
- Explicit operands override `$@`: with `set -- -x` and `getopts a opt -- -a`, the explicit operand path parses `-a` (sets var=`"a"`, exit 0).
- User reset: write `OPTIND="1"` mid-iteration, then call `getopts` again — `scope.getopts_subindex` becomes 0 (verify by stacked-option scenario).

### 6.3 Scope tests (`src/env/vars.rs::tests`)

- `push_scope` with `OPTIND="3"` already set → new scope sees `OPTIND="1"`; `saved_optind == Some("3".to_string())`.
- `pop_scope` writes `OPTIND="3"` back into the (now-current) caller scope.
- Nested `push_scope`/`pop_scope` round-trips both `getopts_subindex` and `saved_optind` independently per scope.

### 6.4 E2E conversion

For each of the 9 XFAIL files, delete the `# XFAIL: …` header line.
No body changes expected. If a test expectation turns out wrong during
implementation, fix it in the same commit and explain in the message
(per roadmap §5.3).

### 6.5 Regression checks

- `getopts_end_with_double_dash.sh` and `getopts_no_more.sh` (currently
  passing via `/usr/bin/getopts` fallback) must remain PASS under the
  native implementation.
- Full `./e2e/run_tests.sh` reports `XFail: 21` (was 30 pre-SP4).
- `cargo test` stays green.

## 7. Out of Scope

| Excluded | Reason |
|---|---|
| optstring leading `-` (GNU reorder) | POSIX defines only `:` prefix |
| Long options `--foo=bar` | POSIX `getopts` is single-letter only |
| External `getopt(1)` | Separate utility, not POSIX-required |
| `OPTERR` variable | bash extension; POSIX has silent mode only |
| `set -x` formatting changes for `getopts` | Existing xtrace coverage is sufficient |
| Plugin-host API for OPTIND/OPTARG/subindex | No plugin use case identified |

## 8. Acceptance Criteria

1. All 9 target XFAIL tests pass under `./e2e/run_tests.sh`.
2. `./e2e/run_tests.sh` summary reports `XFail: 21`.
3. `cargo test` is green across unit + integration tests.
4. No previously passing e2e test regresses (compare baseline output).
5. `TODO.md` `POSIX Required Builtin Implementation` section's
   `getopts` bullet is deleted; the SP4 roadmap line is also deleted
   per the project convention.

## 9. Suggested Commit Shape

Following the SP2/SP3 pattern of independent groups per commit:

1. **G1 — env/vars scope plumbing**
   `Scope { getopts_subindex, saved_optind }` + `push_scope` /
   `pop_scope` updates + `ShellEnv::new` OPTIND init + tests.
2. **G2 — getopts builtin core**
   `src/builtin/getopts.rs` (parse_args + step_getopts + unit/integration tests) +
   `src/builtin/mod.rs` wiring (`BUILTIN_NAMES`, `classify_builtin`,
   `exec_regular_builtin`).
3. **G3 — E2E unblock**
   Remove `# XFAIL:` lines from the 9 target files.
4. **G4 — TODO.md / memory closure**
   Delete the SP4 roadmap line and the `getopts` POSIX-required-builtin
   bullet; update `project_e2e_xfail_roadmap.md` memory to record SP4
   completion and refreshed XFAIL count.

## 10. Open Questions

None at design time. Implementation may surface edge cases (e.g.,
multi-byte option characters); the agreed default is "byte-level
match, ASCII spec only — non-ASCII chars are treated as unknown
options."
