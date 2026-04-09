#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution in double quotes preserves spaces
# EXPECT_OUTPUT: a  b  c
echo "$(echo 'a  b  c')"
