# kish - POSIX Shell in Rust

A POSIX-compliant shell (IEEE Std 1003.1) implemented in Rust.

## Build & Test

```bash
cargo build                          # Debug build
cargo test                           # Unit + integration tests
cargo test --test <name>             # Single test file (e.g., interactive, signals, subshell)
cargo test <test_name>               # Single test by name
./e2e/run_tests.sh                   # E2E POSIX compliance tests (requires debug build)
./e2e/run_tests.sh --filter=<pat>    # Filtered E2E tests
cargo bench                          # Criterion benchmarks
```

## Architecture

Processing pipeline: **Lexer** (`src/lexer/`) -> **Parser** (`src/parser/`) -> **Expander** (`src/expand/`) -> **Executor** (`src/exec/`)

Shell state lives in `ShellEnv` (`src/env/`). Interactive mode is in `src/interactive/`.

## Key Conventions

- **POSIX compliance is the primary goal.** Non-POSIX extensions (e.g., bash-isms) should be explicitly noted and kept separate.
- **Error messages** are prefixed with `kish: ` on stderr.
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
