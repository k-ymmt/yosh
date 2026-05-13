#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Unquoted expansion is split on default IFS (space, tab, newline)
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
x="a b c"
set -- $x
echo $#
