#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: integer comparison operators
# EXPECT_OUTPUT: lt eq ge
# EXPECT_EXIT: 0
[ 1 -lt 2 ] && printf 'lt '
[ 3 -eq 3 ] && printf 'eq '
[ 4 -ge 4 ] && printf 'ge'
echo
