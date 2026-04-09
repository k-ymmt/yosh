#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Undefined variable is treated as 0 in arithmetic
# EXPECT_OUTPUT<<END
# 0
# 5
# END
unset x
echo $((x))
echo $((x + 5))
