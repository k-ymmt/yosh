# yosh

A POSIX-compliant shell (IEEE Std 1003.1-2024) implemented in Rust.

yosh aims for strict POSIX compliance at its core, while layering on a
modern interactive experience and a WebAssembly-based plugin system —
both kept cleanly separate from the standard shell language.

## Features

- **POSIX compliance first.** The lexer, parser, expander, and executor
  target the POSIX Shell Command Language, backed by 1000+ end-to-end
  compliance tests referencing POSIX.1-2024 section numbers
  (see [`e2e/`](e2e/)). Implementation-defined choices are documented in
  [`docs/yosh/posix-compliance.md`](docs/yosh/posix-compliance.md).
- **Interactive shell** with:
  - Real-time syntax highlighting and command validity checking
  - Spec-based tab completion (TOML-defined, bundled for all builtins
    plus common external commands like `git`)
  - Fuzzy history search and interactive selectors
  - Kill ring, undo, and Emacs-style keybindings
- **Plugin system.** Plugins are WebAssembly Components run in
  [wasmtime](https://wasmtime.dev/), with capability-gated access to
  shell state, sandboxed by default. Author plugins in Rust with
  [`yosh-plugin-sdk`](crates/yosh-plugin-sdk/); install them from
  GitHub releases or local files. See
  [`docs/yosh/plugin.md`](docs/yosh/plugin.md).
- **Byte-transparent.** Non-UTF-8 script paths, arguments, and input are
  preserved losslessly end to end.

## Installation

From [crates.io](https://crates.io/crates/yosh):

```sh
cargo install yosh
```

Or build from source:

```sh
git clone https://github.com/k-ymmt/yosh
cd yosh
cargo build --release
# binary at target/release/yosh
```

## Usage

```sh
yosh                     # interactive shell (when stdin is a TTY)
yosh script.sh [args]    # run a script
yosh -c 'echo hello'     # run a command string
echo 'echo hi' | yosh    # read a script from stdin
```

Subcommands:

```sh
yosh plugin --help        # manage plugins (install, sync, update, list, verify)
yosh completions --help   # manage tab-completion specs (export, list)
```

## Configuration

| What | Where |
|------|-------|
| Startup file (interactive) | `$ENV` (parameter-expanded, per POSIX) |
| History | `$HISTFILE` (default `~/.yosh_history`), `$HISTSIZE` / `$HISTFILESIZE` |
| Completion specs | `~/.config/yosh/completions/<command>.toml` (overrides bundled specs) |
| Plugins | `~/.config/yosh/plugins.toml` |

Completion specs bundled with the binary can be exported as a starting
point for customization:

```sh
yosh completions export git   # writes ~/.config/yosh/completions/git.toml
```

The TOML spec format is documented in [`completion.md`](completion.md).

## Architecture

Processing pipeline:

```
Lexer (src/lexer/) → Parser (src/parser/) → Expander (src/expand/) → Executor (src/exec/)
```

Shell state lives in `ShellEnv` (`src/env/`); interactive mode is in
`src/interactive/`. Builtins are split into special and regular per the
POSIX classification (`src/builtin/`).

Workspace crates:

| Crate | Purpose |
|-------|---------|
| `yosh` | The shell itself |
| [`yosh-plugin-api`](crates/yosh-plugin-api/) | WIT-generated plugin bindings |
| [`yosh-plugin-sdk`](crates/yosh-plugin-sdk/) | High-level Rust SDK for plugin authors |
| [`yosh-plugin-manager`](crates/yosh-plugin-manager/) | Plugin install/sync/update logic |

## Development

```sh
cargo build              # debug build
cargo test               # unit + integration tests
./e2e/run_tests.sh       # E2E POSIX compliance tests (requires debug build)
cargo bench              # Criterion benchmarks
```

See [`CLAUDE.md`](CLAUDE.md) for the full development guide, including
how to build the WebAssembly test plugins.

## Exit Codes

Standard POSIX conventions: `0` success, `1` general error, `2`
usage/syntax error, `126` found but not executable, `127` command not
found, `128+N` terminated by signal N.

## License

[MIT](LICENSE)
