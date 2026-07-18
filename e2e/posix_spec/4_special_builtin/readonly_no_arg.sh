#!/bin/sh
# POSIX_REF: 2.15 readonly
# DESCRIPTION: readonly with no args lists read-only variables
# EXPECT_EXIT: 0
readonly myvar2=v2
readonly | grep -q myvar2 || exit 1
