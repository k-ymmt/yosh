#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution inside double quotes
# EXPECT_OUTPUT: result is hello
echo "result is $(echo hello)"
