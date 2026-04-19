#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde directly after a parameter expansion (no colon) stays literal per POSIX
# EXPECT_OUTPUT: /base~/bin
# EXPECT_EXIT: 0
HOME=/home/x
base=/base
x=$base~/bin
echo "$x"
