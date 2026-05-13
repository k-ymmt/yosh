#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Parentheses override operator precedence
# EXPECT_OUTPUT: 14
# EXPECT_EXIT: 0
echo $((2*(3+4)))
