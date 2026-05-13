#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval's concatenation respects word splitting between operands
# EXPECT_OUTPUT: a b
# EXPECT_EXIT: 0
eval echo a b
