#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: eval executes concatenated arguments as shell command
# EXPECT_OUTPUT: hello
eval 'echo hello'
