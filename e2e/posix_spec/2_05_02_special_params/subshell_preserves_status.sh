#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - ?
# DESCRIPTION: Subshell creation preserves the value of $?
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
false
(echo $?)
