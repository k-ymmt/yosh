#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde following unquoted '=' in variable assignment expands
# EXPECT_OUTPUT: /tmp/hdir/bin
# EXPECT_EXIT: 0
HOME=/tmp/hdir
x=~/bin
echo "$x"
