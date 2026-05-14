#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read assigns one line of stdin to a variable
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
echo hello | { read line; echo "$line"; }
