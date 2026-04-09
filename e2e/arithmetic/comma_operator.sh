#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Comma operator evaluates left to right, returns last
# EXPECT_OUTPUT<<END
# 3
# 1
# 2
# END
echo $((a=1, b=2, a+b))
echo "$a"
echo "$b"
