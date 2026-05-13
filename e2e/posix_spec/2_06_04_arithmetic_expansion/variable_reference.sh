#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Bare variable name inside $(()) is evaluated as its numeric value
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
x=5
echo $((x+1))
