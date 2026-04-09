#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Empty command substitution produces empty string
# EXPECT_OUTPUT: -end
x=$()
echo "${x}-end"
