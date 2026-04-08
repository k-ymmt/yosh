#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution
# EXPECT_OUTPUT: hello
echo $(echo $(echo hello))
