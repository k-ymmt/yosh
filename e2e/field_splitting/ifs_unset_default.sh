#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: unset IFS restores default splitting on space/tab/newline
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
IFS=:
unset IFS
x="a b c"
for i in $x; do
  echo "$i"
done
