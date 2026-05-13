# E2E XFAIL Roadmap — Sub-Project Decomposition

**Date:** 2026-05-13
**Status:** Roadmap (no implementation in this spec)
**Type:** Decomposition / planning

## 1. Background

The `e2e/posix_spec/` test suite currently carries **55 XFAIL-marked test
files** spread across nine POSIX sections. The XFAIL reasons fall into three
broad categories:

- **Not yet implemented** (25): missing builtins (`read`, `getopts`,
  `ulimit`), missing variables (`$PPID`, `PS4`, `LANG`-driven locale,
  `OPTIND`/`OPTARG`), missing semantics (`trap 0` on subshell exit,
  redirect-only commands, redirection left-to-right ordering, reserved
  word after assignment prefix).
- **Non-POSIX deviation** (19): existing builtins return wrong exit
  status, emit no diagnostic for invalid input, or apply semantics that
  differ from POSIX (`break`/`continue` outside loop, `unset -f`,
  `readonly -p`, `exec` env propagation, command-substitution `$?` flow,
  no native `type`/`hash`).
- **Harness limitation** (11): tests that rely on interactive
  history, an editor, `/dev/tty`, the default `PS1`, or POSIX semantics
  whose interpretation varies across shells.

Tackling all 55 in a single effort is infeasible: the work spans new
builtin implementations (1–2 weeks each), small bug-fix sweeps,
PTY-harness build-out, and explicit deferral. This document partitions
the 55 tests into seven sub-projects so each can be brainstormed,
planned, and implemented independently.

## 2. Scope

In scope:

- Enumerate every current XFAIL test and assign it to exactly one
  sub-project.
- Define each sub-project's scope, dependencies, and recommended file
  surface.
- Specify the recommended execution order and the rationale.
- Record cross-cutting conventions (spec naming, TODO.md updates, test
  conversion procedure).

Out of scope:

- Designing any individual sub-project's implementation.
- Writing tests; refactoring code; touching `src/`.
- Reclassifying existing test metadata headers (handled per sub-project).

## 3. Sub-Project Catalog

Each sub-project (SP) targets a single `docs/superpowers/specs/…-design.md`
brainstorming pass. Test counts sum to 55 (11 + 5 + 9 + 9 + 8 + 10 + 3).

### SP1 — Special-builtin error diagnostics & semantics (11 tests)

**Files:**

- `e2e/posix_spec/4_special_builtin/break_outside_loop.sh`
- `e2e/posix_spec/4_special_builtin/continue_outside_loop.sh`
- `e2e/posix_spec/4_special_builtin/continue_n_exceeds_depth.sh`
- `e2e/posix_spec/4_special_builtin/unset_invalid_name.sh`
- `e2e/posix_spec/4_special_builtin/unset_f_function.sh`
- `e2e/posix_spec/4_special_builtin/unset_f_keeps_variable.sh`
- `e2e/posix_spec/4_special_builtin/readonly_invalid_name.sh`
- `e2e/posix_spec/4_special_builtin/readonly_p_listing.sh`
- `e2e/posix_spec/4_special_builtin/export_invalid_name.sh`
- `e2e/posix_spec/4_special_builtin/exec_keeps_env.sh`
- `e2e/posix_spec/2_08_01_consequences_of_shell_errors/special_builtin_redir_error_exits.sh`

(`exec_redir_input.sh` originally belonged here but depends on the
`read` builtin from SP3 and was moved during SP1 brainstorming —
see `2026-05-13-e2e-xfail-sp1-special-builtin-design.md` §2.)

**Nature:** Existing-builtin bug fixes — exit code, stderr diagnostic,
or side-effect semantics. Code surface is concentrated in
`src/builtin/special.rs` and `src/exec/simple.rs`.

**Dependencies:** None.

### SP2 — Required-builtin diagnostics + native `type`/`hash` (5 tests)

**Files:**

- `e2e/posix_spec/4_required_builtin/jobs_unknown_spec.sh`
- `e2e/posix_spec/4_required_builtin/jobs_invalid_option.sh`
- `e2e/posix_spec/4_required_builtin/type_alias.sh`
- `e2e/posix_spec/4_required_builtin/type_function.sh`
- `e2e/posix_spec/4_required_builtin/hash_unknown_cmd.sh`

**Nature:** Fix `jobs` exit status for invalid input; replace external
`/usr/bin/type` and `/usr/bin/hash` fallthrough with native builtins
that see yosh's alias/function/hash state. Code surface in
`src/builtin/regular.rs`.

**Dependencies:** None.

### SP3 — `read` builtin implementation (9 tests)

**Files:**

- `e2e/posix_spec/4_required_builtin/read_basic.sh`
- `e2e/posix_spec/4_required_builtin/read_partial_line.sh`
- `e2e/posix_spec/4_required_builtin/read_multiple_vars.sh`
- `e2e/posix_spec/4_required_builtin/read_no_args.sh`
- `e2e/posix_spec/4_required_builtin/read_last_var_gets_remainder.sh`
- `e2e/posix_spec/4_required_builtin/read_r_preserves_backslash.sh`
- `e2e/posix_spec/4_required_builtin/read_strips_ifs.sh`
- `e2e/posix_spec/4_special_builtin/exec_close_fd.sh`
- `e2e/posix_spec/4_special_builtin/exec_redir_input.sh`

**Nature:** New required builtin. Reads one logical line from stdin,
applies IFS field splitting across N variable names, with the last
variable receiving the remainder. `-r` disables backslash escape
processing. `exec_close_fd` and `exec_redir_input` are bundled because
both verify `exec` redirection by calling `read` immediately after,
which only works once `read` is implemented.

**Dependencies:** None.

### SP4 — `getopts` builtin implementation (9 tests)

**Files:**

- `e2e/posix_spec/4_required_builtin/getopts_basic.sh`
- `e2e/posix_spec/4_required_builtin/getopts_with_arg.sh`
- `e2e/posix_spec/4_required_builtin/getopts_stacked.sh`
- `e2e/posix_spec/4_required_builtin/getopts_unknown.sh`
- `e2e/posix_spec/4_required_builtin/getopts_missing_arg.sh`
- `e2e/posix_spec/4_required_builtin/getopts_optind.sh`
- `e2e/posix_spec/8_env_vars/OPTIND_initial_one.sh`
- `e2e/posix_spec/8_env_vars/OPTIND_advances.sh`
- `e2e/posix_spec/8_env_vars/OPTARG_set_by_getopts.sh`

**Nature:** New required builtin. Parses positional parameters against
an option string, updating `OPTIND` and `OPTARG` between calls. Handles
stacked options (`-abc`), missing arguments, unknown options, and the
`:opstring` silent-error variant.

**Dependencies:** Independent from SP3, but the error-handling style
established by SP3 is recommended to be reused.

### SP5 — Miscellaneous small POSIX features (8 tests)

**Files:**

- `e2e/posix_spec/2_04_reserved_words/reserved_after_assignment_recognized.sh`
- `e2e/posix_spec/2_06_03_command_substitution/exit_status_propagates_to_parent.sh`
- `e2e/posix_spec/2_07_redirection/redir_order_left_to_right.sh`
- `e2e/posix_spec/2_09_01_simple_commands/redirection_only_creates_file.sh`
- `e2e/posix_spec/2_11_signals_and_error_handling/trap_zero_runs_on_exit.sh`
- `e2e/posix_spec/4_special_builtin/trap_int_handler.sh`
- `e2e/posix_spec/8_env_vars/PPID_is_set.sh`
- `e2e/posix_spec/8_env_vars/PS4_assigned.sh`

**Nature:** Independent small features and fixes that each touch a
distinct subsystem (parser, expander, redirection layer, trap
machinery, env startup, xtrace formatting). Bundled into one SP because
none warrants its own spec.

**Dependencies:** None among themselves.

### SP6 — PTY harness migration (10 tests)

**Files:**

- `e2e/posix_spec/4_required_builtin/fc_e_editor.sh`
- `e2e/posix_spec/4_required_builtin/fc_l_lists_recent.sh`
- `e2e/posix_spec/4_required_builtin/fc_l_n_no_numbers.sh`
- `e2e/posix_spec/4_required_builtin/fc_no_command.sh`
- `e2e/posix_spec/4_required_builtin/fc_r_reverse.sh`
- `e2e/posix_spec/4_required_builtin/fc_s_substitute.sh`
- `e2e/posix_spec/8_env_vars/FCEDIT_default_ed.sh`
- `e2e/posix_spec/8_env_vars/FCEDIT_used_by_fc.sh`
- `e2e/posix_spec/8_env_vars/PS1_default_value.sh`
- `e2e/posix_spec/4_special_builtin/exec_no_cmd_redirects.sh`

**Nature:** Migrate to the existing `tests/pty_interactive.rs` expectrl
harness so the tests can see interactive history, an editor process,
the default `PS1`, and `/dev/tty`. `fc` scenarios should drive
`FCEDIT=cat` so the editor's "edit" step is deterministic. If a fix in
yosh itself is required to make a PTY test pass, it lands within this
sub-project. Any case that PTY cannot exercise either is demoted to SP7.

**Dependencies:** None; can be scheduled any time but recommended after
SP3–SP5 so the non-interactive harness's limits are well-understood
before investing in the PTY path.

### SP7 — Deferred / recorded as known deviation (3 tests)

**Files:**

- `e2e/posix_spec/4_required_builtin/ulimit_unknown_option.sh`
- `e2e/posix_spec/8_env_vars/LANG_default_collate.sh`
- `e2e/posix_spec/2_11_signals_and_error_handling/trap_resets_in_subshell_when_unhandled.sh`

**Nature:** Explicitly deferred. `ulimit` is a large new builtin
unjustified by current demand; locale handling underpins `LANG` and is
a multi-week scope of its own; trap-reset-in-subshell semantics are not
agreed across POSIX shells. Each XFAIL line is rewritten to one of:

```sh
# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
# XFAIL: deferred (TODO: locale support — tracked in TODO.md)
# XFAIL: known POSIX deviation (trap reset in subshell — interpretation varies across shells)
```

Each is documented in `TODO.md` with a rationale paragraph.

## 4. Recommended Execution Order

| Order | Sub-project | Tests | Rationale |
|-------|-------------|-------|-----------|
| 1 | SP1 | 11 | Small bug fixes establish a working iteration cycle; touches code already under maintenance. |
| 2 | SP2 | 5 | Same shape as SP1; native `type`/`hash` are short. |
| 3 | SP3 | 9 | First large feature; clears `exec_close_fd` and `exec_redir_input` blockers. |
| 4 | SP5 | 8 | Independent small features fit between two large ones. |
| 5 | SP4 | 9 | Larger feature with state (`OPTIND`/`OPTARG` across invocations). |
| 6 | SP6 | 10 | PTY work after non-interactive coverage is maximal. |
| 7 | SP7 | 3 | Pure documentation; can land last. |

The order keeps two heavy implementation SPs (SP3, SP4) separated by a
small-feature interlude (SP5) to spread review load. SP6 is
intentionally last among active work so the team has full visibility
into which cases truly cannot be exercised non-interactively.

## 5. Cross-cutting Conventions

### 5.1 Per-SP spec procedure

When starting a sub-project:

1. Run the `superpowers:brainstorming` skill with this roadmap as
   context.
2. Save the spec to
   `docs/superpowers/specs/YYYY-MM-DD-e2e-xfail-sp<N>-<topic>-design.md`,
   linking back to this roadmap.
3. Define an explicit acceptance criterion: every test file listed in
   the sub-project's catalog must have its `# XFAIL: …` line removed,
   the test must pass under `./e2e/run_tests.sh`, and `cargo test` must
   stay green.

### 5.2 TODO.md

On committing this roadmap, add a `## E2E XFAIL Roadmap` section to
`TODO.md` listing each SP with:

- SP number, title, and test count.
- Pointer to this spec.
- Current status (pending / in progress / completed).

When a sub-project completes, delete its entry per the project
convention "Delete completed items from TODO.md".

### 5.3 Test conversion

Each XFAIL test file already declares the correct
`EXPECT_OUTPUT`/`EXPECT_EXIT`/`EXPECT_STDERR`. The goal per test is to
remove only the `# XFAIL: …` line so the test runs as a normal
expectation. If a test's stated expectation turns out to be wrong
during implementation, fix it in the same commit and explain in the
commit message; do not silently re-introduce XFAIL.

### 5.4 SP6 escape hatch

If a PTY migration attempt reveals that a test cannot be exercised even
via expectrl (e.g., the scenario requires an interactive editor with
human keystrokes), the test is demoted into SP7 with a rewritten XFAIL
comment of the form `# XFAIL: known POSIX deviation (…)` or
`# XFAIL: deferred (…)` and a TODO.md entry. SP6 closes once every
listed test is either green under PTY or explicitly demoted.

## 6. Acceptance Criterion (roadmap level)

This roadmap is complete when all seven sub-projects' acceptance
criteria are met. At that point the e2e suite reports zero XFAIL in
SP1–SP6 and three documented XFAIL entries in SP7, each backed by a
TODO.md rationale.
