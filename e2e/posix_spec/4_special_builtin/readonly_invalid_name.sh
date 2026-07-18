#!/bin/sh
# POSIX_REF: 2.15 readonly
# DESCRIPTION: readonly with identifier starting with digit is an error
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
readonly 1foo=v
