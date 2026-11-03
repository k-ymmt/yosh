#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: The EXIT trap fired by a special-builtin error runs every command of its action
# EXPECT_OUTPUT<<END
# first
# second
# END
# EXPECT_EXIT: 2
exec 2>/dev/null
trap 'echo first; echo second' EXIT
unset -Z x
echo unreachable
