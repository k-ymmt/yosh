#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution inside double-quotes does not field-split
# EXPECT_OUTPUT: a b c
# EXPECT_EXIT: 0
echo "$(echo a b c)"
