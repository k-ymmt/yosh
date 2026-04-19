#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde after a ':' expands even if the first segment has no tilde
# EXPECT_OUTPUT: /usr:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=/usr:~/bin
echo "$x"
