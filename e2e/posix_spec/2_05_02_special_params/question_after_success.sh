#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $? is 0 after a successful command
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
true
echo $?
