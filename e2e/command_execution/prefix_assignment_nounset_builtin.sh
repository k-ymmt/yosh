#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A set -u failure in a builtin's prefix assignment aborts the command and exits the shell
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: zz
set -u
x=$zz echo ran
echo after
