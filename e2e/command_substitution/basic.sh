#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Basic command substitution with $(...)
# EXPECT_OUTPUT: hello
echo $(echo hello)
