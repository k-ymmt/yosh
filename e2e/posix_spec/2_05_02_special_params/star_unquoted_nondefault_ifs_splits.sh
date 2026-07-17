#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - *
# DESCRIPTION: Unquoted $* yields one field per positional parameter even when IFS has no space
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS=:
set -- a b c
n=0
for w in $*; do n=$((n+1)); done
echo $n
