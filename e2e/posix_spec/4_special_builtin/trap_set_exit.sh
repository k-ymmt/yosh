#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap action runs on EXIT
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
trap 'echo bye' EXIT
