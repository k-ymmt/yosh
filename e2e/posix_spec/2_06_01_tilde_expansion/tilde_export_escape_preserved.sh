#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: export NAME=\~/val preserves the backslash-escaped tilde as literal
# EXPECT_OUTPUT: ~/val
# EXPECT_EXIT: 0
HOME=/home/x
export NAME=\~/val
echo "$NAME"
