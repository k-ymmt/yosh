#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Compound assignment operators (+=, -=, etc.)
# EXPECT_OUTPUT<<END
# 15
# 12
# END
x=10
echo $((x += 5))
echo $((x -= 3))
