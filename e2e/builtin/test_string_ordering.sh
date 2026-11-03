#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: POSIX.1-2024 string ordering primaries < and > compare in collation (byte) order
# EXPECT_OUTPUT: 0 1 0 1 0
# EXPECT_EXIT: 0
[ a \< b ]; a=$?
[ b \< a ]; b=$?
test b '>' a; c=$?
test a '>' a; d=$?
test B '<' a; e=$?
echo "$a $b $c $d $e"
