#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A special-builtin error inside the EXIT trap action ends the shell with the error status
# EXPECT_OUTPUT: main
# EXPECT_EXIT: 2
exec 2>/dev/null
trap 'unset -Z x; echo after' EXIT
echo main
true
