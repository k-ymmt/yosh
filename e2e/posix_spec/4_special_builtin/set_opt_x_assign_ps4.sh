#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: PS4 prefix is applied to assignment-only trace lines
# EXPECT_STDERR: > x=1
# EXPECT_EXIT: 0
PS4='> '
set -x
x=1
