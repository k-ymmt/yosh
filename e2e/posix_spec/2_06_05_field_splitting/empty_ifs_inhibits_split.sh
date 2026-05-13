#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Empty IFS inhibits all field splitting
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
IFS=
set -- $(echo "a b c")
echo $#
