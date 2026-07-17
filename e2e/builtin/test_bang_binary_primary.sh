#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: 3-argument test with a binary primary in $2 compares strings, even when $1 is !
# EXPECT_OUTPUT: 1 0
# EXPECT_EXIT: 0
[ ! = x ]; a=$?
[ ! = ! ]; b=$?
echo "$a $b"
