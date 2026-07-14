#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: $' has no special meaning inside double-quotes
# EXPECT_OUTPUT: $'a'
# EXPECT_EXIT: 0
echo "$'a'"
