#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A fatal expansion error inside the EXIT trap action ends the shell with a non-zero status, cutting the action short
# EXPECT_OUTPUT: main
# EXPECT_EXIT: 1
exec 2>/dev/null
unset x
trap ': ${x:?missing}; echo after' EXIT
echo main
true
