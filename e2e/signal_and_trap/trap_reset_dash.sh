#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap - SIGNAL resets handler to default
# EXPECT_OUTPUT: hello
trap 'echo trapped' EXIT
trap - EXIT
echo hello
