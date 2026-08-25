#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Temporary assignment to a readonly variable diagnoses but execution continues
# EXPECT_OUTPUT: continued
# EXPECT_EXIT: 0
# EXPECT_STDERR: readonly
readonly r=1
r=2 true
echo continued
