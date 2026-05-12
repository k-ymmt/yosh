#!/bin/sh
# POSIX_REF: 2.14.14 trap
# DESCRIPTION: EXIT trap fires on non-zero exit
# EXPECT_OUTPUT: cleanup
# EXPECT_EXIT: 1
trap 'echo cleanup' EXIT
exit 1
