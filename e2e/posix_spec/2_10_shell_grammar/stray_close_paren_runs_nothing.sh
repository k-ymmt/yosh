#!/bin/sh
# POSIX_REF: 2.10.2 Shell Grammar Rules
# DESCRIPTION: A stray ) after a command is a syntax error detected before the command runs
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
echo before
x=$(echo a ) echo ran)
echo after
