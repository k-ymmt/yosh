#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: An invalid read option is a usage error with status 2
# EXPECT_OUTPUT: 2
# EXPECT_STDERR: -z
# EXPECT_EXIT: 0
read -z x </dev/null
echo $?
