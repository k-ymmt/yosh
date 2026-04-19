#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands when the preceding segment is a parameter expansion
# EXPECT_OUTPUT: /base:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
base=/base
x=$base:~/bin
echo "$x"
