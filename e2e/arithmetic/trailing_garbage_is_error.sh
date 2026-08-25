#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Trailing garbage after a complete expression is a syntax error
# EXPECT_EXIT: 1
# EXPECT_STDERR: syntax error
echo $((1 2))
