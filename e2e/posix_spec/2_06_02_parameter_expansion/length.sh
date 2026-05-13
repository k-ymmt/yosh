#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${#var} expands to string length of var
# EXPECT_OUTPUT: 5
# EXPECT_EXIT: 0
x=hello
echo "${#x}"
