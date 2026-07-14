#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A leading IFS non-whitespace delimiter yields a leading empty field
# EXPECT_OUTPUT: 2:[][a]
# EXPECT_EXIT: 0
IFS=:
v=":a"
set -- $v
echo "$#:[$1][$2]"
