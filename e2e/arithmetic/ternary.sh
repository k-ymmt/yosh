#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Ternary conditional operator
# EXPECT_OUTPUT<<END
# 10
# 20
# END
echo $((1 ? 10 : 20))
echo $((0 ? 10 : 20))
