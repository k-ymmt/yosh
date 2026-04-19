#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands when the preceding segment is a command substitution
# EXPECT_OUTPUT: foo:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=$(echo foo):~/bin
echo "$x"
