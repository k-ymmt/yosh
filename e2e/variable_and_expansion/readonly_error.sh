#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment to readonly variable produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly x=hello
x=world
