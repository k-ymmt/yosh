#!/bin/sh
# POSIX_REF: 2.14.4 colon
# DESCRIPTION: colon builtin returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
:
echo $?
