#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: string = and != comparisons
# EXPECT_OUTPUT: eq neq
# EXPECT_EXIT: 0
[ "abc" = "abc" ] && printf 'eq '
[ "abc" != "xyz" ] && printf 'neq'
echo
