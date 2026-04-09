#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for loop without in clause iterates over "$@"
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
set -- a b c
for i; do
  echo "$i"
done
