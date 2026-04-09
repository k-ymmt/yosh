#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Mixed whitespace and non-whitespace IFS characters
# EXPECT_OUTPUT<<END
# one
# two
# three
# END
IFS=": "
x="one: two:three"
for i in $x; do
  echo "$i"
done
