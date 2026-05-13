#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: IFS='' (empty) means no word splitting
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
IFS=''
v="a b c"
set -- $v
echo "$1"
