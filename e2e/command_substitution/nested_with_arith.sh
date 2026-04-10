#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Arithmetic expansion inside command substitution
# EXPECT_OUTPUT: 3
echo $(echo $((1+2)))
