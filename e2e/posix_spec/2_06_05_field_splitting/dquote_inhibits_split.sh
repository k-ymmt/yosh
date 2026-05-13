#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Double-quoted expansion is not split
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
x="a b c"
set -- "$x"
echo $#
