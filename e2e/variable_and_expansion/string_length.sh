#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: String length with ${#var}
# EXPECT_OUTPUT: 5
x=hello
echo "${#x}"
