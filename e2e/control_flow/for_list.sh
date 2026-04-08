#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for loop iterates over word list
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
for i in a b c; do
  echo "$i"
done
