#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: exec without command does not replace the shell
# EXPECT_OUTPUT: still here
exec
echo still here
