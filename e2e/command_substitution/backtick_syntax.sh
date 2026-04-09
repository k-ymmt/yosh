#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Backtick syntax for command substitution
# EXPECT_OUTPUT: hello
echo `echo hello`
