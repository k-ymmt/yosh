#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Empty IFS disables field splitting
# EXPECT_OUTPUT: a:b:c
IFS=
x="a:b:c"
for i in $x; do
  echo "$i"
done
