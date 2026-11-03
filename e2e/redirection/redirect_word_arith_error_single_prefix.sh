#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Arithmetic error in a redirection word exits the shell; the diagnostic has a single yosh: prefix
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh: arithmetic: division by zero
echo > $((1/0))
echo after
