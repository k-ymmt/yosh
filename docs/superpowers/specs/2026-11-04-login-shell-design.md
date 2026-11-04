# Login Shell (`-l`) Design

## Context

yosh has no notion of a login shell. The leading-option loop in
`src/main.rs` rejects `-l` as an invalid option, and the only startup
files it reads are the interactive ones: `~/.yoshrc` and then the file
named by `$ENV` (`src/interactive/startup.rs`, `source_startup_files`).
Nothing reads `/etc/profile` or `~/.profile`, so yosh cannot be used as
a login shell (`chsh`, a terminal application's "run as login shell"
setting, or an explicit `yosh -l`) without losing the PATH and
environment those files set up — on macOS, `/etc/profile` is what runs
`path_helper`.

POSIX does not define `-l`; it is a widely shared extension. The
reference behaviours differ:

| Shell | Trigger | Files read | `$-` |
|-------|---------|------------|------|
| dash | `-l`, or argv[0] starting with `-` | `/etc/profile`, then `~/.profile`; `$ENV` afterwards if interactive | reports `l` |
| bash | `-l` / `--login`, or argv[0] starting with `-` | `/etc/profile`, then the first readable of `~/.bash_profile`, `~/.bash_login`, `~/.profile`; `~/.bash_logout` at exit | no letter (`shopt login_shell`) |
| zsh | `-l`, or argv[0] starting with `-` | zsh-specific names only (`zprofile` → `zshrc` → `zlogin`, `zlogout` at exit); `/etc/profile` only in `sh` emulation | reports `l` |

## Decisions

All of these were settled with the user before implementation.

1. **Meaning.** `-l` makes yosh a login shell, in the dash/bash/zsh
   sense.
2. **Files.** A login shell reads `/etc/profile`, then `$HOME/.profile`.
   No yosh-specific profile file and no bash-style search list.
3. **Interactive and non-interactive alike.** The profile files are
   read whenever the shell is a login shell, regardless of how commands
   arrive: the REPL, `yosh -l script.sh`, `yosh -l -c cmd`, and
   `yosh -l` on a piped stdin all read them. dash and bash agree.
4. **argv[0].** A leading `-` in argv[0] (`-yosh`, as `login(1)` and
   terminal applications exec a login shell) also makes the shell a
   login shell. `$0` keeps the dash: it reports argv[0] unchanged, as
   dash, bash and zsh do.
5. **`+l`.** `+l` turns the login behaviour off. The option is
   tri-state like `-i`/`+i`: unspecified means "decide from argv[0]",
   and the last of `-l`/`+l` on the command line wins, also over an
   argv[0] that starts with `-`.
6. **Order with the interactive files.** An interactive login shell
   reads `/etc/profile` → `~/.profile` → `~/.yoshrc` → `$ENV`. Profile
   first, as zsh (`zprofile` before `zshrc`) and dash (profile before
   `$ENV`) do, so a PATH assembled in a profile is visible to the rc
   file. The interactive files stay interactive-only (POSIX XCU sh:
   `$ENV` is read by interactive shells only); `-l` does not make a
   non-interactive shell read them.
7. **Timing.** The profile files are read where the interactive startup
   files are read today: after the invocation options, job control,
   prompt and history defaults, and plugin loading, and immediately
   before `~/.yoshrc`. In the non-interactive `run_string` path they
   are read after plugin loading and before the first command.
8. **Errors.** Same handling as `~/.yoshrc`: a missing file is skipped
   silently (`Executor::source_file` returns `None`); a syntax error is
   reported with the file name and the shell goes on; `exit` inside a
   profile ends the shell there with that status, and the files after
   it are not read. A non-interactive login shell keeps the ordinary
   non-interactive fatal-error rules for what runs inside the profile.
9. **`$-`.** The flag string reports `l`, as dash does, so scripts can
   test `case $- in *l*)`. `l` is invocation state, not a settable
   option: `set -l`, `set +l` and `set -o login` stay errors, and `set
   +o` output does not list it — the same treatment as the `c` and `s`
   letters. It is emitted in the existing alphabetical slot, between
   `f` and `m` (`a b c C e f i l m n s u v x`), and subshells and
   command substitutions keep it because `ShellOptions` is cloned into
   them.
10. **Not in scope.** `--login` (yosh accepts `--long` words only for
    its own `--help`/`--version`/`--parse` dispatch), and a logout file
    read when a login shell exits (`~/.zlogout`/`~/.bash_logout`
    analogue). The logout file is recorded in `TODO.md`.
11. **Tests and `/etc/profile`.** Tests set `HOME` to a temporary
    directory and verify through `~/.profile`; `/etc/profile` is read
    from the real system, with no test-only override. On macOS it only
    runs `path_helper` and checks `$BASH`, so it is harmless to the
    assertions, which look at `$-` and at marker output from
    `~/.profile`.

## Design

### Option state

`ShellOptions` (`src/env/shell_mode.rs`) gains `pub login: bool`,
documented next to `cmd_string` and `stdin_reads` as invocation-only
state reported in `$-`. `to_flag_string` emits `l` after `f` and before
`m`. `set_by_char`/`set_by_name` do not learn the letter, so `set`
keeps rejecting it. `ShellMode::flag_string` inserts the `i` snapshot
before the first of `l m n s u v x` so `i` stays ahead of `l`.

### Invocation parsing

In `src/main.rs`, the leading-option loop gets a tri-state
`login: Option<bool>` next to `force_interactive`, set by the new arm
`'l' => login = Some(on)` ahead of the `set_by_char` fallback. After
the loop, the effective value is

```
let login = login.unwrap_or(shell_name.starts_with('-'));
```

where `shell_name` is argv[0] as already captured for `$0`. The flag is
passed into `run_string`, `run_file`, `run_stdin` and `Repl::new`
alongside the existing `interactive`/`invocation_ops` arguments and
stored in `executor.env.mode.options.login` before any startup file
runs, so `$-` is correct inside the profile files too.

The help table lists `-l` ("Act as a login shell: read /etc/profile
and ~/.profile"), and the usage line printed by
`invocation_usage_error` adds `l` to both clusters.

### Reading the profile files

`src/interactive/startup.rs` gains

```
pub fn source_login_files(executor: &mut Executor)
```

which sources `/etc/profile` and then `$HOME/.profile` with
`Executor::source_file`, skipping the second when `HOME` is empty or
when the first set `exit_requested`. It lives beside
`source_startup_files` and is re-exported from `interactive::mod` the
same way.

Call sites:

- `Repl::new` (interactive REPL): after `load_plugins(true)`, call
  `source_login_files` when the shell is a login shell, then
  `source_startup_files` unless `exit_requested` is already set. The
  existing check in `Repl::run` ends the shell before the first prompt.
- `run_string` (`-c`, script file, piped stdin, and `-i` on a
  non-terminal): after `load_plugins`, set the prompt defaults when
  interactive, then `source_login_files` when login, returning the
  `exit_requested` code if set, then the existing interactive
  `source_startup_files` block.

Nothing changes for a shell that is neither login nor interactive.

### Documentation

- `README.md`: a `yosh -l` line in the Usage block and a "Login shell
  files" row in the Configuration table.
- `docs/yosh/posix-compliance.md`: a new "Invocation: login shells
  (`-l`)" section recording the behaviour as a non-POSIX extension and
  the dash alignment.
- `TODO.md`: the deferred logout file.

## Tests

`tests/invocation.rs` (piped stdin, temporary `HOME`):

- `-l` reads `~/.profile` and `$-` contains `l`; without `-l` neither
  happens.
- `-l -c cmd` and `-l script.sh` read `~/.profile`.
- `-il` reads `~/.profile`, then `~/.yoshrc`, then `$ENV`, in that
  order.
- `+l` after `-l` cancels it; `-l` after `+l` enables it.
- argv[0] `-yosh` (via `CommandExt::arg0`) makes a login shell with
  `$0` still `-yosh`; `-yosh +l` is not a login shell.
- `exit 7` in `~/.profile` ends the shell with status 7 before
  `~/.yoshrc` runs.
- A syntax error in `~/.profile` is reported with the file name and
  the shell continues.
- `set -l` is still an error.

`src/env/shell_mode.rs` unit test: the flag string with `login`,
interactive snapshot and `monitor` set reads `ilm` in that order.

`tests/pty_interactive.rs`: one test spawning `yosh -l` on a PTY with
`~/.profile` and `~/.yoshrc` each echoing a marker, asserting the
profile marker precedes the rc marker before the first prompt.
