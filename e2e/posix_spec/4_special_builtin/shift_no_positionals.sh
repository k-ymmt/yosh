#!/bin/sh
# POSIX_REF: 2.15 shift
# DESCRIPTION: shift when $# is 0 is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: shift
set --
shift
