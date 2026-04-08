#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: while loop iterates until condition becomes false
# EXPECT_OUTPUT<<END
# 0
# 1
# 2
# END
x=0
while test "$x" -lt 3; do
  echo "$x"
  x=$((x + 1))
done
