#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A trailing IFS non-whitespace delimiter does not create a trailing empty field
# EXPECT_OUTPUT: 1:[a]
# EXPECT_EXIT: 0
IFS=:
v="a:"
set -- $v
echo "$#:[$1]"
