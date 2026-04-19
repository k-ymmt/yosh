#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde that is not at segment start stays literal
# EXPECT_OUTPUT: foo~/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=foo~/bin
echo "$x"
