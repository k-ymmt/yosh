#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character
# DESCRIPTION: Backslash-newline is line continuation
# EXPECT_OUTPUT: helloworld
echo hello\
world
