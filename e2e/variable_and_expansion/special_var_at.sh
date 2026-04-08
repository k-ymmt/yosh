#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: "$@" expands to each positional parameter as separate field
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
set -- a b c
for i in "$@"; do
  echo "$i"
done
