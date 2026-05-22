#!/bin/sh
# POSIX E2E Test Runner for yosh
# Discovers and runs .sh test files under e2e/, comparing actual output
# against metadata expectations (EXPECT_OUTPUT, EXPECT_EXIT, EXPECT_STDERR).
#
# ── CI / Toolchain notes ─────────────────────────────────────────────────────
# When adding GitHub Actions CI, include these steps before `cargo test`:
#
#   - name: Install wasm32-wasip2 target
#     run: rustup target add wasm32-wasip2
#
#   - name: Cache cargo-component
#     uses: actions/cache@v4
#     with:
#       path: ~/.cargo/bin/cargo-component
#       key: cargo-component-0.18.0
#
#   - name: Install cargo-component
#     run: |
#       if ! command -v cargo-component >/dev/null; then
#         cargo install cargo-component --locked --version 0.18.0
#       fi
#
#   - name: Build wasm test plugins
#     run: |
#       cargo component build -p test_plugin --target wasm32-wasip2 --release
#       cargo component build -p trap_plugin --target wasm32-wasip2 --release
#
#   - name: Run tests
#     # NOTE: do NOT use `--workspace` here. The wasm-component test plugins
#     # under tests/plugins/* are excluded from `default-members`; trying to
#     # build them on the host target fails with undefined wit-bindgen cabi
#     # symbols. The default-members invocation covers everything that
#     # actually has host-runnable tests.
#     run: cargo test --features test-helpers
#
# Local tooling: mise.toml pins cargo-component = 0.18.0.
# ─────────────────────────────────────────────────────────────────────────────

set -u

# ── Defaults ──────────────────────────────────────────────────────────
SHELL_UNDER_TEST="./target/debug/yosh"
FILTER=""
VERBOSE=0
# Per-test wall-clock budget. Override with YOSH_E2E_TIMEOUT — release.sh sets
# a higher value because its parallel-load run can stall any single test under
# CPU contention well past the standalone budget.
TIMEOUT="${YOSH_E2E_TIMEOUT:-15}"

# ── Auto failure log ─────────────────────────────────────────────────
# When any test FAILS or TIMES OUT, append details (path, kind, captured
# stdout/stderr) to this file. The file is created lazily on first failure,
# so clean runs don't litter /tmp with empty logs. Override with
# YOSH_E2E_FAILURE_LOG=/path/to/log if you want a stable filename.
FAILURES_LOG="${YOSH_E2E_FAILURE_LOG:-${TMPDIR:-/tmp}/e2e-failures-$(date +%Y%m%d-%H%M%S).log}"
_failures_log_created=0

# Append one failure record to $FAILURES_LOG.
# Args: $1=rel_path  $2=kind (FAIL|TIMEOUT)  $3=reason  $4=stdout_file  $5=stderr_file
append_failure_log() {
    _fl_path="$1"
    _fl_kind="$2"
    _fl_reason="$3"
    _fl_stdout="$4"
    _fl_stderr="$5"

    if [ "$_failures_log_created" = 0 ]; then
        : > "$FAILURES_LOG" || return 0
        {
            printf "# yosh e2e failure log\n"
            printf "# Started: %s\n" "$(date '+%Y-%m-%d %H:%M:%S')"
            printf "# Shell:   %s\n" "$SHELL_UNDER_TEST"
            printf "# Filter:  %s\n" "${FILTER:-<none>}"
            printf "\n"
        } >> "$FAILURES_LOG"
        _failures_log_created=1
    fi

    {
        printf "==== [%s] %s ====\n" "$_fl_kind" "$_fl_path"
        printf "Reason: %s\n" "$_fl_reason"
        printf "%s\n" "-- stdout --"
        if [ -f "$_fl_stdout" ]; then
            cat "$_fl_stdout"
            # Ensure trailing newline for readability
            [ -s "$_fl_stdout" ] && printf "\n"
        fi
        printf "%s\n" "-- stderr --"
        if [ -f "$_fl_stderr" ]; then
            cat "$_fl_stderr"
            [ -s "$_fl_stderr" ] && printf "\n"
        fi
        printf "\n"
    } >> "$FAILURES_LOG"
}

# ── Color codes ───────────────────────────────────────────────────────
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    RED='' GREEN='' YELLOW='' CYAN='' BOLD='' RESET=''
fi

# ── Usage ─────────────────────────────────────────────────────────────
usage() {
    printf "Usage: %s [OPTIONS]\n" "$0"
    printf "Options:\n"
    printf "  --shell=PATH     Shell to test (default: %s)\n" "$SHELL_UNDER_TEST"
    printf "  --filter=PATTERN Only run tests whose path contains PATTERN\n"
    printf "  --verbose        Show detailed output for each test\n"
    printf "  --help           Show this help\n"
    printf "\nEnvironment:\n"
    printf "  YOSH_E2E_NO_TIMEOUT=1  Skip per-test timeout; never set in CI or\n"
    printf "                         release.sh (individual runaway tests will hang forever)\n"
    printf "  YOSH_E2E_TIMEOUT=N     Override per-test timeout (default: 15s)\n"
    exit 0
}

# ── Parse arguments ───────────────────────────────────────────────────
for arg in "$@"; do
    case "$arg" in
        --shell=*)   SHELL_UNDER_TEST="${arg#--shell=}" ;;
        --filter=*)  FILTER="${arg#--filter=}" ;;
        --verbose)   VERBOSE=1 ;;
        --help)      usage ;;
        *)           printf "Unknown option: %s\n" "$arg" >&2; exit 1 ;;
    esac
done

# ── Verify shell exists ──────────────────────────────────────────────
if [ ! -x "$SHELL_UNDER_TEST" ]; then
    printf "Error: shell not found or not executable: %s\n" "$SHELL_UNDER_TEST" >&2
    exit 1
fi

# ── Build wasm test plugins ──────────────────────────────────────────
if ! command -v cargo-component >/dev/null 2>&1; then
    printf "WARN: cargo-component not installed, skipping wasm test-plugin builds\n"
    printf "       run: cargo install cargo-component --locked --version 0.18.0\n"
else
    printf ">>> Building wasm test plugins\n"
    cargo component build -p test_plugin --target wasm32-wasip2 --release
    cargo component build -p trap_plugin --target wasm32-wasip2 --release
fi

# ── Isolated HOME ────────────────────────────────────────────────────
# yosh reads ~/.config/yosh/plugins.lock on every startup. Without isolation,
# the user's plugin config (e.g. broken or large entries) loads on each of the
# hundreds of test invocations, causing per-test overhead and flakiness under
# parallel load. Point HOME at an empty temp dir so the runner is hermetic.
ISOLATED_HOME="$(mktemp -d "${TMPDIR:-/tmp}/yosh_e2e_home.XXXXXX")"
trap 'rm -rf "$ISOLATED_HOME"' EXIT INT TERM
export HOME="$ISOLATED_HOME"

# ── Locate e2e directory ─────────────────────────────────────────────
E2E_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── Counters ─────────────────────────────────────────────────────────
total=0
passed=0
failed=0
xfailed=0
xpassed=0
timedout=0
migrated=0

# ── Parse metadata from a test file ─────────────────────────────────
# Sets: meta_posix_ref, meta_description, meta_expect_output,
#       meta_expect_exit, meta_expect_stderr, meta_xfail,
#       meta_migrated, meta_has_expect_output
parse_metadata() {
    _file="$1"
    meta_posix_ref=""
    meta_description=""
    meta_expect_output=""
    meta_expect_exit="0"
    meta_expect_stderr=""
    meta_xfail=""
    meta_migrated=""
    meta_has_expect_output=0

    _in_heredoc=0
    _heredoc_delim=""
    _heredoc_buf=""
    _heredoc_first=0

    while IFS= read -r _line; do
        # Inside a heredoc block
        if [ "$_in_heredoc" = 1 ]; then
            # Check for end delimiter: must be "# DELIM" exactly
            _stripped="${_line#"# "}"
            if [ "$_stripped" = "$_heredoc_delim" ]; then
                _in_heredoc=0
                meta_expect_output="$_heredoc_buf"
                meta_has_expect_output=1
                continue
            fi
            # Append line (strip leading "# ")
            if [ "$_heredoc_first" = 1 ]; then
                _heredoc_buf="$_stripped"
                _heredoc_first=0
            else
                _heredoc_buf="${_heredoc_buf}
${_stripped}"
            fi
            continue
        fi

        case "$_line" in
            "# POSIX_REF: "*)
                meta_posix_ref="${_line#"# POSIX_REF: "}"
                ;;
            "# DESCRIPTION: "*)
                meta_description="${_line#"# DESCRIPTION: "}"
                ;;
            "# EXPECT_OUTPUT<<"*)
                # Multi-line heredoc style: # EXPECT_OUTPUT<<DELIM
                _heredoc_delim="${_line#"# EXPECT_OUTPUT<<"}"
                _in_heredoc=1
                _heredoc_buf=""
                _heredoc_first=1
                ;;
            "# EXPECT_OUTPUT:"|"# EXPECT_OUTPUT: "*)
                meta_expect_output="${_line#"# EXPECT_OUTPUT:"}"
                meta_expect_output="${meta_expect_output# }"
                meta_has_expect_output=1
                ;;
            "# EXPECT_EXIT: "*)
                meta_expect_exit="${_line#"# EXPECT_EXIT: "}"
                ;;
            "# EXPECT_STDERR: "*)
                meta_expect_stderr="${_line#"# EXPECT_STDERR: "}"
                ;;
            "# XFAIL: "*)
                meta_xfail="${_line#"# XFAIL: "}"
                ;;
            "# MIGRATED_TO: "*)
                meta_migrated="${_line#"# MIGRATED_TO: "}"
                ;;
        esac
    done < "$_file"

    if [ "$_in_heredoc" = 1 ]; then
        printf "Warning: unclosed EXPECT_OUTPUT heredoc (delimiter '%s') in %s\n" \
            "$_heredoc_delim" "$_file" >&2
    fi
}

# ── Collect test files ───────────────────────────────────────────────
# IMPORTANT: Use $(find ...) to avoid subshell from pipe, so counters persist.
test_files=$(find "$E2E_DIR" -name '*.sh' -not -name 'run_tests.sh' -type f | sort)

# ── Main test loop ───────────────────────────────────────────────────
for test_file in $test_files; do
    # Compute relative path for display
    rel_path="${test_file#"$E2E_DIR/"}"

    # Apply filter
    if [ -n "$FILTER" ]; then
        case "$rel_path" in
            *"$FILTER"*) ;;
            *) continue ;;
        esac
    fi

    total=$((total + 1))

    # Parse metadata
    parse_metadata "$test_file"

    # Migrated tests: short-circuit (no execution, no temp dir).
    if [ -n "$meta_migrated" ]; then
        if [ -n "$meta_xfail" ]; then
            printf "${YELLOW}[WARN]${RESET}  %s has both MIGRATED_TO and XFAIL — MIGRATED_TO wins; remove the stale XFAIL\n" "$rel_path"
        fi
        migrated=$((migrated + 1))
        printf "${CYAN}[MIGRATED]${RESET} %s (%s)\n" "$rel_path" "$meta_migrated"
        continue
    fi

    # Create per-test temp directory
    TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/yosh_e2e.XXXXXX")
    export TEST_TMPDIR

    # Run the test with timeout
    actual_stdout=""
    actual_stderr=""
    actual_exit=0

    _stdout_file="$TEST_TMPDIR/_stdout"
    _stderr_file="$TEST_TMPDIR/_stderr"
    _exit_file="$TEST_TMPDIR/_exit"

    # Use a background process + wait to implement timeout.
    # Wrap the test shell with a perl one-liner that resets SIGINT/SIGQUIT to
    # SIG_DFL before exec, so the test shell does not inherit SIG_IGN from this
    # background subshell (POSIX §2.11: a shell that inherits SIG_IGN cannot
    # trap or reset that signal).  Plain `trap - INT QUIT` inside a bash
    # background subshell does not override the inherited SIG_IGN at the kernel
    # level; perl's `$SIG{X}="DEFAULT"` does.
    (
        perl -e '$SIG{INT}="DEFAULT";$SIG{QUIT}="DEFAULT";exec @ARGV or die $!' \
            -- "$SHELL_UNDER_TEST" "$test_file" >"$_stdout_file" 2>"$_stderr_file"
    ) &
    _pid=$!

    # Timeout logic: single-shot sleep + kill.
    # Set YOSH_E2E_NO_TIMEOUT=1 to skip the timer entirely (local fast runs).
    if [ "${YOSH_E2E_NO_TIMEOUT:-0}" = "1" ]; then
        _timer_pid=""
    else
        # Single-shot watchdog: SIGKILL the test if it outlives $TIMEOUT.
        # Benign race — if the test exits just as the timer fires, kill -9
        # returns ESRCH and we skip writing the "timeout" marker. The exit
        # code from `wait $_pid` below is the authoritative result; the
        # marker branch is diagnostic only, so the race cannot corrupt
        # pass/fail accounting.
        (
            sleep "$TIMEOUT"
            kill -9 "$_pid" 2>/dev/null && echo "timeout" >"$_exit_file"
        ) &
        _timer_pid=$!
    fi

    wait "$_pid" 2>/dev/null
    _wait_status=$?
    if [ -n "$_timer_pid" ]; then
        kill "$_timer_pid" 2>/dev/null
        wait "$_timer_pid" 2>/dev/null
    fi

    # Read results — exit code from wait, timeout from marker file
    actual_exit=$_wait_status
    if [ -f "$_exit_file" ] && [ "$(cat "$_exit_file")" = "timeout" ]; then
        actual_exit="timeout"
    fi

    if [ -f "$_stdout_file" ]; then
        actual_stdout=$(cat "$_stdout_file")
    fi

    if [ -f "$_stderr_file" ]; then
        actual_stderr=$(cat "$_stderr_file")
    fi

    # ── Determine pass/fail ──────────────────────────────────────
    _test_ok=1
    _failure_reason=""

    # Check for timeout
    if [ "$actual_exit" = "timeout" ]; then
        _test_ok=0
        _failure_reason="Timed out after ${TIMEOUT}s"
    else
        # Check exit code
        if [ "$actual_exit" != "$meta_expect_exit" ]; then
            _test_ok=0
            _failure_reason="Exit code: expected=$meta_expect_exit actual=$actual_exit"
        fi

        # Check stdout (exact match, trailing newline normalized)
        if [ "$meta_has_expect_output" = 1 ]; then
            _norm_expected=$(printf '%s' "$meta_expect_output")
            _norm_actual=$(printf '%s' "$actual_stdout")
            if [ "$_norm_expected" != "$_norm_actual" ]; then
                _test_ok=0
                if [ -n "$_failure_reason" ]; then
                    _failure_reason="$_failure_reason; "
                fi
                _failure_reason="${_failure_reason}Stdout mismatch"
            fi
        fi

        # Check stderr (substring match)
        if [ -n "$meta_expect_stderr" ]; then
            case "$actual_stderr" in
                *"$meta_expect_stderr"*) ;;
                *)
                    _test_ok=0
                    if [ -n "$_failure_reason" ]; then
                        _failure_reason="$_failure_reason; "
                    fi
                    _failure_reason="${_failure_reason}Stderr: expected substring '$meta_expect_stderr' not found"
                    ;;
            esac
        fi
    fi

    # ── Report result ────────────────────────────────────────────
    if [ "$actual_exit" = "timeout" ]; then
        timedout=$((timedout + 1))
        printf "${YELLOW}[TIME]${RESET}  %s\n" "$rel_path"
        printf "        Timed out after ${TIMEOUT}s\n"
        append_failure_log "$rel_path" "TIMEOUT" "$_failure_reason" \
            "$_stdout_file" "$_stderr_file"
    elif [ -n "$meta_xfail" ]; then
        # Expected failure
        if [ "$_test_ok" = 1 ]; then
            xpassed=$((xpassed + 1))
            printf "${YELLOW}[XPASS]${RESET} %s (expected failure: %s)\n" "$rel_path" "$meta_xfail"
        else
            xfailed=$((xfailed + 1))
            printf "${CYAN}[XFAIL]${RESET} %s (%s)\n" "$rel_path" "$meta_xfail"
        fi
    else
        if [ "$_test_ok" = 1 ]; then
            passed=$((passed + 1))
            printf "${GREEN}[PASS]${RESET}  %s\n" "$rel_path"
        else
            failed=$((failed + 1))
            printf "${RED}[FAIL]${RESET}  %s\n" "$rel_path"
            printf "        %s\n" "$_failure_reason"
            append_failure_log "$rel_path" "FAIL" "$_failure_reason" \
                "$_stdout_file" "$_stderr_file"
        fi
    fi

    # Verbose output
    if [ "$VERBOSE" = 1 ]; then
        printf "        ${BOLD}Description:${RESET} %s\n" "${meta_description:-<none>}"
        [ -n "$meta_posix_ref" ] && printf "        ${BOLD}POSIX ref:${RESET}   %s\n" "$meta_posix_ref"
        if [ "$actual_exit" != "timeout" ]; then
            printf "        ${BOLD}Exit code:${RESET}   %s (expected %s)\n" "$actual_exit" "$meta_expect_exit"
            if [ "$meta_has_expect_output" = 1 ]; then
                printf "        ${BOLD}Expected stdout:${RESET}\n"
                printf "          |%s\n" "$meta_expect_output"
                printf "        ${BOLD}Actual stdout:${RESET}\n"
                printf "          |%s\n" "$actual_stdout"
            fi
            if [ -n "$meta_expect_stderr" ]; then
                printf "        ${BOLD}Expected stderr substring:${RESET} %s\n" "$meta_expect_stderr"
                printf "        ${BOLD}Actual stderr:${RESET} %s\n" "$actual_stderr"
            fi
        fi
        printf "\n"
    fi

    # Clean up temp directory
    rm -rf "$TEST_TMPDIR"
done

# ── Summary ──────────────────────────────────────────────────────────
printf "\n${BOLD}── Summary ──${RESET}\n"
printf "Total: %d  " "$total"
printf "${GREEN}Passed: %d${RESET}  " "$passed"
printf "${RED}Failed: %d${RESET}  " "$failed"
printf "${YELLOW}Timedout: %d${RESET}  " "$timedout"
printf "${CYAN}XFail: %d${RESET}  " "$xfailed"
printf "${CYAN}Migrated: %d${RESET}  " "$migrated"
printf "${YELLOW}XPass: %d${RESET}\n" "$xpassed"

# Point users at the auto-captured failure log if one was written.
if [ "$_failures_log_created" = 1 ]; then
    printf "\n${YELLOW}Failure details captured in:${RESET} %s\n" "$FAILURES_LOG"
fi

# Exit code: 0 if no failures (XPASS and timedout count as failures too)
if [ "$failed" -gt 0 ] || [ "$xpassed" -gt 0 ] || [ "$timedout" -gt 0 ]; then
    exit 1
fi
exit 0
