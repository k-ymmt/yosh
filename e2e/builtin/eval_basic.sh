#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval executes concatenated arguments as shell command
# EXPECT_OUTPUT: hello
eval 'echo hello'
