#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: a trap set inside a subshell does not leak to the parent
# EXPECT_OUTPUT: parent
# EXPECT_EXIT: 0
( trap 'echo subshell' EXIT; true )
trap 'echo parent' EXIT
