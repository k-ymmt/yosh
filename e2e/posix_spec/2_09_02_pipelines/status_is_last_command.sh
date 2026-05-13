#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline exit status is that of the last command
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
false | true
echo $?
