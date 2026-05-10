#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: backslash-newline is a line continuation (token splice)
# EXPECT_OUTPUT: ab
# EXPECT_EXIT: 0
echo a\
b
