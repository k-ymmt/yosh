#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set with operands and no -- sets the positional parameters
# EXPECT_OUTPUT: 3 b
# EXPECT_EXIT: 0
set a b c
echo $# $2
