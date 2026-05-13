#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap can set the same action for multiple signals
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
trap 'echo bye' EXIT TERM INT
