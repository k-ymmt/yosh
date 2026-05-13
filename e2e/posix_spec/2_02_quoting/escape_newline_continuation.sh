#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash-newline is line continuation, removed from input
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
echo a\
b
