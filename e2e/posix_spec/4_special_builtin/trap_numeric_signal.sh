#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap accepts numeric signal numbers
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
trap 'echo bye' 0
