#!/bin/sh
# POSIX_REF: 2.15 exit
# DESCRIPTION: EXIT trap action sees the explicit exit status in $?
# EXPECT_OUTPUT: t=7
# EXPECT_EXIT: 7
trap 'echo t=$?' EXIT
exit 7
