#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Compound assignment operators (+=, -=, etc.)
# XFAIL: Arithmetic compound assignment operators not implemented
# EXPECT_OUTPUT<<END
# 15
# 12
# END
x=10
echo $((x += 5))
echo $((x -= 3))
