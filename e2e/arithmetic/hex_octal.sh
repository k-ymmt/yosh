#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Hexadecimal and octal literals
# EXPECT_OUTPUT<<END
# 255
# 8
# END
echo $((0xFF))
echo $((010))
