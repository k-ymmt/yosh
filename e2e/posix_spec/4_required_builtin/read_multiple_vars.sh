#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with multiple var names splits the line by IFS
# EXPECT_OUTPUT: a-b-c
# EXPECT_EXIT: 0
echo a b c | { read x y z; echo "$x-$y-$z"; }
