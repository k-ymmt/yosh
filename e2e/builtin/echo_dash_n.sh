#!/bin/sh
# POSIX_REF: 4 Utilities - echo
# DESCRIPTION: echo -n suppresses trailing newline
# EXPECT_OUTPUT: helloworld
echo -n hello
echo world
