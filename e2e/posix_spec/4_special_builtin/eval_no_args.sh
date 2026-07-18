#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval with no operands returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
eval
echo $?
