#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Assigning a readonly variable inside $((...)) is an expansion error that aborts the script
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
readonly x=1
echo before
echo $((x=5))
echo after
