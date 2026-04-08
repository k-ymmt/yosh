#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Variable reference with braces
# EXPECT_OUTPUT: helloworld
x=hello
echo "${x}world"
