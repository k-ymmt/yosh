#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: readonly variable cannot be modified
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
x=hello
readonly x
x=world
