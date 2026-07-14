#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap with a numeric first operand resets the condition (trap 15 = trap - 15)
# EXPECT_OUTPUT: reset-ok
# EXPECT_EXIT: 0
trap 'echo t' TERM
trap 15
trap -p > "$TEST_TMPDIR/traps"
if grep -q SIGTERM "$TEST_TMPDIR/traps"; then echo still-trapped; else echo reset-ok; fi
