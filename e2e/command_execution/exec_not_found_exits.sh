#!/bin/sh
# POSIX_REF: 2.14 exec
# DESCRIPTION: exec failure (not found) exits a non-interactive shell with 127
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 127
# EXPECT_STDERR: not found
echo before
exec /nonexistent_yosh_exec_test_xyz
echo survived
