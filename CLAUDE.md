# yosh - POSIX Shell in Rust

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

### Plugin development

```sh
cargo install cargo-component --locked --version 0.18.0    # one-time
rustup target add wasm32-wasip2                            # one-time
cargo component build -p test_plugin --target wasm32-wasip2 --release
```

Run plugin integration tests with the `test-helpers` feature:

```sh
cargo test --features test-helpers
```

The wasm-component test plugins under `tests/plugins/` are workspace
members but are excluded from `default-members`, so plain `cargo build`
and `cargo test` skip them. Build the wasm artefacts explicitly when
the integration tests need them:

```sh
cargo component build -p test_plugin --target wasm32-wasip2 --release
cargo component build -p trap_plugin --target wasm32-wasip2 --release
```

Avoid `cargo build --workspace` and `cargo test --workspace` — both
attempt to host-build the wasm crates and fail with undefined
wit-bindgen cabi symbols.

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
