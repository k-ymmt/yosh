#!/bin/sh
# POSIX_REF: 2.14.1 Patterns Matching a Single Character
# DESCRIPTION: A collating symbol [.].] is a valid range end point and does not close the bracket expression
# EXPECT_OUTPUT<<END
# match
# match
# no
# END
# EXPECT_EXIT: 0
for c in M ']' a; do
  case $c in [A-[.].]]) echo match;; *) echo no;; esac
done
