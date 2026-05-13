#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: when input has more fields than vars, last var gets the remainder
# XFAIL: not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: a-b c d
# EXPECT_EXIT: 0
echo a b c d | { read x y; echo "$x-$y"; }
