#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Trailing newlines are removed from command substitution
# EXPECT_OUTPUT: xhellox
echo "x$(echo hello)x"
