#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: Successive LINENO expansions yield strictly increasing values
# EXPECT_EXIT: 0
a=$LINENO
b=$LINENO
c=$LINENO
test "$a" -lt "$b" || { echo "a=$a !< b=$b" >&2; exit 1; }
test "$b" -lt "$c" || { echo "b=$b !< c=$c" >&2; exit 1; }
