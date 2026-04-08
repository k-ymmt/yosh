#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function definition and invocation
# EXPECT_OUTPUT: hello
greet() { echo hello; }
greet
