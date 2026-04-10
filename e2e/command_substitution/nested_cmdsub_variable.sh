#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution with variable expansion
# EXPECT_OUTPUT: hello
x=hello
echo $(echo $(echo $x))
