#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Assignment-only command has exit status 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
x=value
echo $?
