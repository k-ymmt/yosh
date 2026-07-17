#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Invalid UTF-8 bytes in captured output are preserved byte-identically
# EXPECT_OUTPUT: 61e962
# EXPECT_EXIT: 0
v=$(printf 'a\351b')
printf '%s' "$v" | od -An -tx1 | tr -d ' \n'
echo
