#!/bin/sh
# POSIX_REF: 2.14.9 readonly
# DESCRIPTION: readonly variable cannot be modified
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
x=hello
readonly x
x=world
