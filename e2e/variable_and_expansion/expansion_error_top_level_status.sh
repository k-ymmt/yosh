#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A nounset failure in an assignment-only command exits the script with status 1, not 0
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: zz
set -u
x=$zz
echo after
