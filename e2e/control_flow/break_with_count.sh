#!/bin/sh
# POSIX_REF: 2.14.4 break
# DESCRIPTION: break N exits N enclosing loops
# EXPECT_OUTPUT: 1-a
set -- a b c
for i in 1 2 3; do
  for j in a b c; do
    echo "${i}-${j}"
    break 2
  done
done
