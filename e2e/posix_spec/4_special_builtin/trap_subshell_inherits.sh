#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: subshell starts with the parent's traps; resetting before subshell loses them
# EXPECT_OUTPUT: inner
# EXPECT_EXIT: 0
trap 'echo inner' EXIT
( true )
