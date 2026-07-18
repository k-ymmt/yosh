#!/bin/sh
# POSIX_REF: 2.15 readonly (XBD 12.2 Guideline 10)
# DESCRIPTION: readonly -- ends options; trailing -p is a bad identifier
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
readonly -- -p
