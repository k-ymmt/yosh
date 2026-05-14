#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec with no command applies input redirection to shell
# EXPECT_OUTPUT: line1
# EXPECT_EXIT: 0
echo line1 > "$TEST_TMPDIR/in"
exec < "$TEST_TMPDIR/in"
read line
echo "$line"
