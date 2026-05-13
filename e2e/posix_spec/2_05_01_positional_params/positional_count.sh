#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $# expands to the number of positional parameters
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
set -- a b c
echo $#
