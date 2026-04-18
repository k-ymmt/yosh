#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: ':?' expansion error in a non-interactive shell terminates the shell
# EXPECT_EXIT: 1
# EXPECT_STDERR: required
unset FOO
: "${FOO:?required}"
echo "unreachable"
