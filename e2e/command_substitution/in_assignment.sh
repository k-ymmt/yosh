#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution result assigned to variable
# EXPECT_OUTPUT: hello
x=$(echo hello)
echo "$x"
