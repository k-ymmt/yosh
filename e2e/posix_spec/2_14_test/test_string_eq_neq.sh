#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: string = and != comparisons
# EXPECT_OUTPUT: eq neq
# EXPECT_EXIT: 0
[ "abc" = "abc" ] && printf 'eq '
[ "abc" != "xyz" ] && printf 'neq'
echo
