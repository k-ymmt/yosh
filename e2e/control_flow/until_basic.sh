#!/bin/sh
# POSIX_REF: 2.9.4.4 until Loop
# DESCRIPTION: until loop iterates until condition becomes true
# EXPECT_OUTPUT<<END
# 0
# 1
# 2
# END
x=0
until test "$x" -ge 3; do
  echo "$x"
  x=$((x + 1))
done
