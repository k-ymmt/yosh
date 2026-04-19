#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: readonly with a tilde RHS expands the tilde
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
readonly RO=~/bin
echo "$RO"
