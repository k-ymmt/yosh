# getopts OPTIND Reset Design

## Context

`TODO.md` records a POSIX compliance gap in the native `getopts`
implementation: a user can restart option parsing by assigning
`OPTIND=1`, but yosh does not currently detect that write when the
previous visible value was already `1`. This matters for stacked options
such as `-ab`: after reading `a`, yosh stores the remaining position in
an internal `getopts_subindex`; assigning `OPTIND=1` must discard that
internal cursor so the next `getopts` starts from the beginning again.

The design goal is to treat `OPTIND=1` as a user reset event, not merely
as a variable value. The implementation will preserve existing
`getopts` parsing behaviour except for the reset semantics.

## Chosen Approach

Track successful writes to `OPTIND` in `VarStore`, where the existing
`getopts_subindex` already lives. Each scope will carry a small write
generation counter for `OPTIND`, plus a record of the generation that
`getopts` has already observed.

When `VarStore::set("OPTIND", value)` succeeds, it advances the current
scope's `OPTIND` write generation even if the assigned value is identical
to the old value. Failed writes, including readonly failures, do not
advance the generation.

At the start of `builtin_getopts`, yosh checks whether the current
scope's `OPTIND` write generation is newer than the generation last
observed by `getopts`. If so, it resets `getopts_subindex` to `0` before
reading the current `OPTIND` value. After `getopts` completes its own
state update, it records the current generation as observed so its own
`OPTIND` update is not mistaken for a user reset on the next call.

## Components

`src/env/vars.rs` owns the new state. `Scope` gains per-scope `OPTIND`
write tracking, initialized alongside `getopts_subindex`. New methods on
`VarStore` expose intent-level operations such as:

- detecting whether `OPTIND` has been written since the last `getopts`
  observation
- marking the current `OPTIND` generation as observed by `getopts`
- resetting the current `getopts_subindex`

The fields remain private. Callers must not manipulate generation
counters directly.

`src/builtin/getopts.rs` consumes the new API. Its parsing algorithm and
`GetoptsStep` data model remain unchanged. The builtin only adds a
pre-parse reset check and a post-update observation mark.

## Data Flow

Without user intervention, stacked options keep their current behaviour.
For `-ab`, the first `getopts` returns `a`, leaves `OPTIND=1`, and stores
`getopts_subindex=2`; the second call returns `b`.

With a user reset, the sequence changes:

1. `getopts` reads `a` from `-ab`.
2. The script assigns `OPTIND=1`.
3. `VarStore::set` records a new `OPTIND` write generation.
4. The next `getopts` sees an unobserved write generation and resets
   `getopts_subindex` to `0`.
5. Parsing restarts at the beginning of argv element 1 and returns `a`
   again.

This distinguishes "the variable value is still 1" from "the user wrote
OPTIND again".

## Error Handling And Compatibility

Readonly handling remains atomic. `builtin_getopts` already pre-checks
the target variable, `OPTARG`, and `OPTIND` for readonly status before
mutating state. That contract remains: if a readonly target would fail,
`getopts` returns the existing error status without partially updating
`opt`, `OPTARG`, `OPTIND`, `getopts_subindex`, or observation state.

Invalid `OPTIND` values keep the current interpretation: non-numeric,
zero, or otherwise unusable values fall back to `1` for parsing. The new
write tracking only decides whether to clear the stacked-option cursor;
it does not broaden or tighten value parsing.

Function scopes keep independent getopts state. `push_scope` continues
to initialize a fresh `OPTIND=1` and `getopts_subindex=0` for the
function body. A function-local `OPTIND=1` reset affects that current
scope only. `pop_scope` continues to restore the caller's saved `OPTIND`
without treating that internal restore as a new user reset in the caller.

## Testing

Add a focused unit test in `src/builtin/getopts.rs` for the POSIX reset
case:

- positional params are `["-ab"]`
- first `getopts ab opt` returns `a`
- script-level `OPTIND=1` assignment is simulated through `VarStore::set`
- second `getopts ab opt` returns `a` again

Keep or add a companion unit test proving that `getopts`' own `OPTIND`
updates do not trigger a reset. For `["-ab"]` with no user assignment,
the first call returns `a` and the second returns `b`.

Add one e2e test under `e2e/posix_spec/4_required_builtin/`, for example
`getopts_optind_reset_stacked.sh`, to cover the shell-visible behaviour.
The e2e must use standard metadata headers and file mode `644`.

## Acceptance Criteria

- Assigning `OPTIND=1` between `getopts` calls resets stacked-option
  parsing even when the visible value was already `1`.
- Existing stacked-option behaviour is unchanged when the user does not
  assign `OPTIND`.
- Readonly failure paths remain non-partial.
- Function scopes keep independent `getopts` reset tracking.
- Unit tests and the focused e2e test pass.
