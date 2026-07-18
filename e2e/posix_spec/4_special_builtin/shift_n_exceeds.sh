#!/bin/sh
# POSIX_REF: 2.15 shift
# DESCRIPTION: shift with n greater than $# is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: shift
set -- a b
shift 5
