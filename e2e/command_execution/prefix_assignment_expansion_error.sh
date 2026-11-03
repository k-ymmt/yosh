#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: An expansion error in a prefix assignment aborts the command and exits the shell
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: m
x=${zz:?m} /bin/echo ran
echo after
