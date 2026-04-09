#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Nested ternary conditional operator
# EXPECT_OUTPUT<<END
# 1
# 2
# 3
# END
a=1
b=1
echo $((a ? b ? 1 : 2 : 3))
b=0
echo $((a ? b ? 1 : 2 : 3))
a=0
echo $((a ? b ? 1 : 2 : 3))
