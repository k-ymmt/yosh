#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: The longest operator token wins: '||' is a single token, not two '|'
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
echo a||echo b
