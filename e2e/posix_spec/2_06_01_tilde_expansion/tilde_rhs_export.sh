#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: export with a tilde RHS expands the tilde
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
export MYVAR=~/bin
echo "$MYVAR"
