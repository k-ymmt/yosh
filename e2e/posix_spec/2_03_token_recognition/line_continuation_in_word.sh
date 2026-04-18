#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Backslash-newline is removed before tokenization
# EXPECT_OUTPUT: helloworld
# EXPECT_EXIT: 0
echo hello\
world
