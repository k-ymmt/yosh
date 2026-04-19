#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Backslash-escaped tilde is not expanded
# EXPECT_OUTPUT: ~/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=\~/bin
echo "$x"
