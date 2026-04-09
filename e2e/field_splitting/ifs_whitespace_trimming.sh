#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: IFS whitespace trims leading and trailing
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
x="  a  b  c  "
for i in $x; do
  echo "$i"
done
