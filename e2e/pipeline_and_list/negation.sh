#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! negates the exit status of a pipeline
# EXPECT_OUTPUT<<END
# 0
# 1
# END
! false
echo "$?"
! true
echo "$?"
