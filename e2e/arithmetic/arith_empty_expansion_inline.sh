#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Empty/unset expansion substitutes empty text inline (no 0 injection); blank whole expression is 0
# EXPECT_OUTPUT<<END
# 12
# 0
# 0
# 0
# END
unset x
echo "$((1${x}2))"
echo "$(($x))"
x=
echo "$((x))"
echo "$(( ))"
