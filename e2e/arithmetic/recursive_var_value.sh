#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Variable value is recursively evaluated as an expression
# EXPECT_OUTPUT<<END
# 3
# 4
# END
x=1+2
echo $((x))
echo $((x + 1))
