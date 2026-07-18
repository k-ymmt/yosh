#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap - EXIT resets EXIT trap to default
# EXPECT_OUTPUT: hello
trap 'echo goodbye' EXIT
trap - EXIT
echo hello
