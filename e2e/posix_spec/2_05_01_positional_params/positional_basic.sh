#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $1 $2 $3 access first three positional parameters
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
set -- a b c
echo "$1 $2 $3"
