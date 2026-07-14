#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! applied to a succeeding pipeline yields exactly 1
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
! true | true
echo $?
