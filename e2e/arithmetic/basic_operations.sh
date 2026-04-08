#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Basic arithmetic operations +, -, *, /
# EXPECT_OUTPUT<<END
# 5
# 1
# 6
# 3
# END
echo $((2 + 3))
echo $((5 - 4))
echo $((2 * 3))
echo $((7 / 2))
