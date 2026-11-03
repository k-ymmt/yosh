#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A prefix assignment to a readonly variable on a function call exits a non-interactive shell
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly x=1
f() { echo ran; }
x=2 f
echo after
