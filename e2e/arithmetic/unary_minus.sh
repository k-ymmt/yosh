#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Unary minus and plus operators
# EXPECT_OUTPUT<<END
# -5
# 3
# -7
# END
echo $((-5))
echo $((+3))
echo $((-(3 + 4)))
