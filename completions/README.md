# Bundled completion specs

Spec-based tab-completion definitions for yosh's POSIX builtin
commands, plus a few common external commands (`git`), in the TOML
format described in `completion.md` at the repository root.

yosh loads specs from `~/.config/yosh/completions/<command>.toml`.
These files are not read from the repository — copy them into place
to use them:

```sh
mkdir -p ~/.config/yosh/completions
cp completions/*.toml ~/.config/yosh/completions/
```

## Conventions used here

- Operands that are free input (variable names, pids, history
  numbers, octal modes, ...) use `type = "none"` so that Tab does not
  fall back to path completion where a path can never be right.
- Operands that name commands (`type`, `hash`, `command`, `exec`,
  `eval`) use `type = "command"`.
- `test.toml` and `[.toml` are identical: file operators (`-f`, `-d`,
  ...) complete their operand as a path.
- Flag lists and `set -o` / `trap` / `kill -s` candidate values match
  what yosh actually implements (see `src/builtin/`, `src/signal.rs`,
  `src/env/shell_mode.rs`), not the full POSIX surface.
- External-command specs (`git.toml`) cover common porcelain
  subcommands and flags, not the full CLI surface; dynamic candidates
  (branches, remotes, tags, stashes) use `exec` sources so they stay
  fresh.

## Deliberately absent

- `.` (dot) — the spec loader refuses the names `.` and `..`, so a
  `..toml` file would never be loaded. Its operand is a file, which
  default path completion already handles.
- `:` (colon) — ignores its operands; no completion is useful.

Validity of every file in this directory is enforced by the
`bundled_completion_specs_parse` unit test in
`src/interactive/spec_completion.rs`.
