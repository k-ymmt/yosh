#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: ?: evaluates only the selected branch; && and || short-circuit their right operand
# EXPECT_OUTPUT<<END
# 2 0
# 3 0
# 0 0
# 1 0
# 1 5
# 0 0
# 1
# END
# EXPECT_EXIT: 0
x=0; echo $((1 ? 2 : (x=5))) $x
x=0; echo $((0 ? (x=5) : 3)) $x
x=0; echo $((0 && (x=5))) $x
x=0; echo $((1 || (x=5))) $x
x=0; echo $((0 || (x=5))) $x
x=0; echo $((0 && 1/0)) $x
echo $((1 || 1%0))
