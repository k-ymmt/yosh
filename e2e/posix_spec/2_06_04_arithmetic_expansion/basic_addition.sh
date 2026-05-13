#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: $((expr)) evaluates arithmetic with addition
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
echo $((1+1))
