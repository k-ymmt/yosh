#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: unset removes a variable
# EXPECT_EXIT: 0
x=hello
test "$x" = "hello" || exit 1
unset x
test -z "$x" || exit 1
