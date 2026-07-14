#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - *
# DESCRIPTION: Unquoted $* undergoes field splitting
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
set -- "a b" c
n=0
for w in $*; do n=$((n+1)); done
echo $n
