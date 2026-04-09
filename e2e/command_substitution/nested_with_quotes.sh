#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution with quotes
# EXPECT_OUTPUT: inner value
echo "$(echo "$(echo 'inner value')")"
