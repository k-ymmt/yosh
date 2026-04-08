#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Default IFS splits on space, tab, newline
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
x="a b c"
for i in $x; do
  echo "$i"
done
