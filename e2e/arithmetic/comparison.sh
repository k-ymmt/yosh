#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Comparison operators return 1 (true) or 0 (false)
# EXPECT_OUTPUT<<END
# 1
# 0
# 1
# 1
# END
echo $((3 > 2))
echo $((2 > 3))
echo $((3 >= 3))
echo $((2 != 3))
