#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! reserved word negates the exit status of a pipeline
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
! false
echo $?
