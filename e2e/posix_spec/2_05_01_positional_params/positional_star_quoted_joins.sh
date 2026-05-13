#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: "$*" joins positional parameters with first IFS character
# EXPECT_OUTPUT: a,b,c
# EXPECT_EXIT: 0
set -- a b c
IFS=,
echo "$*"
