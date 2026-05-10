# POSIX E2E Test Suite

End-to-end tests verifying yosh's POSIX shell compliance against
IEEE Std 1003.1 (POSIX Shell Command Language).

## Running Tests

```sh
# Build yosh first
cargo build

# Run all tests
./e2e/run_tests.sh

# Run with verbose output (shows diff on failure)
./e2e/run_tests.sh --verbose

# Run only tests matching a pattern
./e2e/run_tests.sh --filter=variable

# Run with a different shell for comparison
./e2e/run_tests.sh --shell=/bin/dash
```

## Writing Tests

Each test is a single `.sh` file with metadata comments:

```sh
#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value expansion with :-
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset x
echo "${x:-hello}"
```

### Metadata Fields

| Field | Required | Description |
|---|---|---|
| `POSIX_REF` | Yes | POSIX spec section reference |
| `DESCRIPTION` | Yes | One-line test description |
| `EXPECT_OUTPUT` | No | Expected stdout (exact match) |
| `EXPECT_EXIT` | No | Expected exit code (default: 0) |
| `EXPECT_STDERR` | No | Expected stderr substring |
| `XFAIL` | No | Expected failure reason |

### Multi-line Expected Output

```sh
# EXPECT_OUTPUT<<END
# line1
# line2
# END
```

### Test Environment

- `$TEST_TMPDIR` — per-test temporary directory (auto-cleaned)
- Tests have a 5-second timeout

## Directory Structure

Tests are organized by functional category:

- `command_execution/` — Simple commands, PATH search
- `variable_and_expansion/` — Parameter expansion
- `arithmetic/` — Arithmetic expansion
- `command_substitution/` — Command substitution
- `pipeline_and_list/` — Pipelines, AND/OR lists
- `redirection/` — Redirection operators, heredocs
- `control_flow/` — if, for, while, until, case
- `function/` — Function definition and invocation
- `builtin/` — Special and regular builtins
- `signal_and_trap/` — Signal handling, trap
- `subshell/` — Subshell environment isolation
- `quoting/` — Quoting rules
- `field_splitting/` — Field splitting, pathname expansion

## Result Classification

- **PASS** — Test succeeded as expected
- **FAIL** — Unexpected failure
- **XFAIL** — Expected failure (known limitation)
- **XPASS** — XFAIL test unexpectedly passed (possible fix)

## POSIX_REF Format Contract

Test files declare which POSIX clause they pin via the `POSIX_REF`
metadata line. The accepted shapes are:

- `2.X.Y <Section Name>` — for ordinary section references.
  Example: `POSIX_REF: 2.6.1 Tilde Expansion`
- `2.10.2 Rule N - <Name>` — for tests that pin a specific grammar rule.
  Example: `POSIX_REF: 2.10.2 Rule 1 - First Word`
- `2.10.2 Rule N - <Name> (<discriminator>)` — for multi-case rules.
  Example: `POSIX_REF: 2.10.2 Rule 10 - Keyword (after pipe)`
- `2.10 Shell Grammar - <Topic>` — for cross-rule grammar topics.
  Example: `POSIX_REF: 2.10 Shell Grammar - Terminator Equality`

A naive grep for one shape misses the others. To enumerate all
§2.10.2-related tests, use:

```sh
grep -RE 'POSIX_REF: 2\.10' e2e/posix_spec/
```

### Rule 9 Taxonomy

The label "Rule 9" in test names covers literal POSIX Rule 9 (function
body) plus its grammar-level generalizations: compound_command body and
compound_list body. Generalized cases carry a parenthetical
`<ctx>` discriminator (e.g., `Rule 9 - Body of compound_list (if-then)`)
to disambiguate them from the literal Rule 9 (`Rule 9 - Body of
function`).
