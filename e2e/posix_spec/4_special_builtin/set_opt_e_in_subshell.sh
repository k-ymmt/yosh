#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -e inside a subshell exits only the subshell
# EXPECT_OUTPUT<<END
# subshell-before
# after
# END
# EXPECT_EXIT: 0
( set -e; echo subshell-before; false; echo subshell-after ) 2>/dev/null
echo after
