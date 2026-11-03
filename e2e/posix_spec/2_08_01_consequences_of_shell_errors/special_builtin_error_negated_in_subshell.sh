#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A special-builtin error exits the subshell with its error status even when negated by !, so set -e then exits the parent
# EXPECT_OUTPUT: sub=2
# EXPECT_EXIT: 2
exec 2>/dev/null
(! unset -Z x); echo "sub=$?"
set -e
( ! unset -Z x )
echo survived
