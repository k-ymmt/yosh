#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: After unset LINENO, the next command re-sets it
# EXPECT_EXIT: 0
unset LINENO
x=$LINENO
test -n "$x" || { echo "LINENO was empty after re-setting" >&2; exit 1; }
