#!/bin/sh
# POSIX_REF: 2.10.2 Rule 8 - NAME in function
# DESCRIPTION: A function name cannot start with a digit
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
1f() { :; }
