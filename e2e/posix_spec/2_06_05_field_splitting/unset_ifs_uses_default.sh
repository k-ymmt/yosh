#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Unset IFS behaves as if IFS=<space><tab><newline>
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
unset IFS
x="a b c"
set -- $x
echo $#
