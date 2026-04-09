#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: Unsetting a readonly variable produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly MY_RO_VAR=test
unset MY_RO_VAR
