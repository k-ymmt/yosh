#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: A lexical error on the very first token is a syntax error, not an empty program
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
"abc
