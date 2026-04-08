#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Multiplication has higher precedence than addition
# EXPECT_OUTPUT: 14
echo $((2 + 3 * 4))
