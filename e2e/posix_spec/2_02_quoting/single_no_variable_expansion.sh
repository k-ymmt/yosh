#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Single-quotes suppress variable expansion
# EXPECT_OUTPUT: $x*
# EXPECT_EXIT: 0
x=v
echo '$x*'
