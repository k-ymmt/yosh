#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word after command name is an argument, not a keyword
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
# NOTE: If `if` were recognized as a reserved word in non-command position,
# parsing would fall into an incomplete if-statement and yield a syntax
# error (exit 2) instead of printing the literal `if`.
echo if
