#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: break N exits N levels of nested loops
# EXPECT_OUTPUT: 1a
for i in 1 2; do
  for j in a b c; do
    if test "$j" = b; then break 2; fi
    echo "$i$j"
  done
done
