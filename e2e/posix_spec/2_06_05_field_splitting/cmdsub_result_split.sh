#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Field splitting applies to unquoted command substitution results
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- $(echo "a b")
echo $#
