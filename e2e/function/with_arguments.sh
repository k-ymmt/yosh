#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function receives positional parameters
# EXPECT_OUTPUT: hello world
greet() { echo "hello $1"; }
greet world
