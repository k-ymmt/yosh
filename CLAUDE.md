# yosh - POSIX Shell in Rust

A POSIX-compliant shell (IEEE Std 1003.1) implemented in Rust.

## Build & Test

This project uses [cargo-nextest](https://nexte.st/) for unit + integration tests.
Install via mise (see `mise.local.toml`) by running `mise install`, or manually
with `curl -LsSf https://get.nexte.st/latest/mac | tar zxf - -C $CARGO_HOME/bin`.

```bash
cargo build                              # Debug build
cargo nextest run --workspace            # Unit + integration tests
cargo nextest run --test <name>          # Single test binary (e.g., interactive, signals, subshell)
cargo nextest run -E 'test(<pat>)'       # Filter by test name using the nextest filterset DSL
cargo test --doc --workspace             # Doctests (nextest does not support doctests)
./e2e/run_tests.sh                       # E2E POSIX compliance tests (requires debug build)
./e2e/run_tests.sh --filter=<pat>        # Filtered E2E tests
cargo bench                              # Criterion benchmarks
```

Test configuration lives in `.config/nextest.toml`. The `pty_interactive`
binary is serialized via a `max-threads = 1` test group because its expectrl-based
tests share PTY state.

## Architecture

Processing pipeline: **Lexer** (`src/lexer/`) -> **Parser** (`src/parser/`) -> **Expander** (`src/expand/`) -> **Executor** (`src/exec/`)

Shell state lives in `ShellEnv` (`src/env/`). Interactive mode is in `src/interactive/`.

## Key Conventions

- **POSIX compliance is the primary goal.** Non-POSIX extensions (e.g., bash-isms) should be explicitly noted and kept separate.
- **Error messages** are prefixed with `yosh: ` on stderr.
- **Exit codes:** 0 success, 1 general error, 2 usage/syntax, 126 not executable, 127 not found, 128+N signal.
- **Builtins** are split into special (`src/builtin/special.rs`) and regular (`src/builtin/regular.rs`) per POSIX classification.

## E2E Test Format

Tests in `e2e/` use metadata headers:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value expansion
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
echo "${x:-hello}"
```

E2E test files should have `644` permissions, not `755`.

## TODO.md

Track known limitations and future work in `TODO.md`. **Delete completed items** rather than marking them with `[x]`.

## PTY Tests

PTY-based interactive tests (`tests/pty_interactive.rs`) use the `expectrl` crate. These tests can be flaky in CI due to timing; use generous timeouts.
