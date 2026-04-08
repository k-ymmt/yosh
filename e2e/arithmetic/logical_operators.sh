#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Logical AND and OR operators
# EXPECT_OUTPUT<<END
# 1
# 0
# 1
# 0
# END
echo $((1 && 1))
echo $((1 && 0))
echo $((0 || 1))
echo $((0 || 0))
