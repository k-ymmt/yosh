#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -- with no further operands clears positional parameters
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
set -- a b c
set --
echo $#
