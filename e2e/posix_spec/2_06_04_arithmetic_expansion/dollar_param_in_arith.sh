#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: $-prefixed parameter expansion works inside $(( ))
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
x=5
echo $(( $x + 1 ))
