#!/bin/sh
# POSIX E2E Test Runner for kish
# Discovers and runs .sh test files under e2e/, comparing actual output
# against metadata expectations (EXPECT_OUTPUT, EXPECT_EXIT, EXPECT_STDERR).

set -u

# ── Defaults ──────────────────────────────────────────────────────────
SHELL_UNDER_TEST="./target/debug/kish"
FILTER=""
VERBOSE=0
TIMEOUT=5

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

# ── Locate e2e directory ─────────────────────────────────────────────
E2E_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── Counters ─────────────────────────────────────────────────────────
total=0
passed=0
failed=0
xfailed=0
xpassed=0

# ── Parse metadata from a test file ─────────────────────────────────
# Sets: meta_posix_ref, meta_description, meta_expect_output,
#       meta_expect_exit, meta_expect_stderr, meta_xfail,
#       meta_has_expect_output
parse_metadata() {
    _file="$1"
    meta_posix_ref=""
    meta_description=""
    meta_expect_output=""
    meta_expect_exit="0"
    meta_expect_stderr=""
    meta_xfail=""
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
            "# EXPECT_OUTPUT: "*)
                meta_expect_output="${_line#"# EXPECT_OUTPUT: "}"
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

    # Create per-test temp directory
    TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/kish_e2e.XXXXXX")
    export TEST_TMPDIR

    # Run the test with timeout
    actual_stdout=""
    actual_stderr=""
    actual_exit=0

    _stdout_file="$TEST_TMPDIR/_stdout"
    _stderr_file="$TEST_TMPDIR/_stderr"
    _exit_file="$TEST_TMPDIR/_exit"

    # Use a background process + wait to implement timeout
    (
        exec "$SHELL_UNDER_TEST" "$test_file" >"$_stdout_file" 2>"$_stderr_file"
    ) &
    _pid=$!

    # Timeout logic
    (
        _elapsed=0
        while [ "$_elapsed" -lt "$TIMEOUT" ]; do
            sleep 1
            _elapsed=$((_elapsed + 1))
            # Check if process is still running
            if ! kill -0 "$_pid" 2>/dev/null; then
                exit 0
            fi
        done
        # Timed out — kill the process
        kill -9 "$_pid" 2>/dev/null
        echo "timeout" >"$_exit_file"
    ) &
    _timer_pid=$!

    wait "$_pid" 2>/dev/null
    _wait_status=$?
    kill "$_timer_pid" 2>/dev/null
    wait "$_timer_pid" 2>/dev/null

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
    if [ -n "$meta_xfail" ]; then
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
        fi
    fi

    # Verbose output
    if [ "$VERBOSE" = 1 ]; then
        printf "        ${BOLD}Description:${RESET} %s\n" "${meta_description:-<none>}"
        [ -n "$meta_posix_ref" ] && printf "        ${BOLD}POSIX ref:${RESET}   %s\n" "$meta_posix_ref"
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
printf "${CYAN}XFail: %d${RESET}  " "$xfailed"
printf "${YELLOW}XPass: %d${RESET}\n" "$xpassed"

# Exit code: 0 if no failures (XPASS counts as failure too)
if [ "$failed" -gt 0 ] || [ "$xpassed" -gt 0 ]; then
    exit 1
fi
exit 0
