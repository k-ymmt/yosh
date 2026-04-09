#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: cd in subshell does not affect parent cwd
# EXPECT_EXIT: 0
original=$(pwd)
(cd /tmp)
current=$(pwd)
test "$original" = "$current"
