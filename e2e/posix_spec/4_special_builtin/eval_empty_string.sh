#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval of an empty string returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
eval ""
echo $?
