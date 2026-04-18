#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Pipe operator terminates the preceding word without whitespace
# EXPECT_OUTPUT: hallo
# EXPECT_EXIT: 0
echo hello|tr e a
