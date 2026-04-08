#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for without in-clause iterates over positional params
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
set -- a b c
for i; do
  echo "$i"
done
