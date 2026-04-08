#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap - EXIT resets EXIT trap to default
# EXPECT_OUTPUT: hello
trap 'echo goodbye' EXIT
trap - EXIT
echo hello
