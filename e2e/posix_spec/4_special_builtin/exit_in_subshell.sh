#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit inside a subshell exits only the subshell
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
( exit 5 )
echo after
