#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: ( E ) grouping around 1- and 2-operand forms
# EXPECT_OUTPUT: one two
# EXPECT_EXIT: 0
[ \( "x" \) ] && printf 'one '
[ \( -n "x" \) ] && printf 'two'
echo
