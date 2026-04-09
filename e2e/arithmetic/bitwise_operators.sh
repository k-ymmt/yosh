#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Bitwise operators &, |, ^, ~, <<, >>
# EXPECT_OUTPUT<<END
# 4
# 7
# 3
# -6
# 20
# 2
# END
echo $((5 & 6))
echo $((5 | 6))
echo $((5 ^ 6))
echo $((~5))
echo $((5 << 2))
echo $((10 >> 2))
