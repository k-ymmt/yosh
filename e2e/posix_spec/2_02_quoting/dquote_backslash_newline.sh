#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash-newline inside double-quotes is a line continuation
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
echo "a\
b"
