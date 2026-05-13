#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: -c option executes the argument string and exits
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
./target/debug/yosh -c 'echo hello'
