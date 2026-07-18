#!/bin/sh
# POSIX_REF: 2.15 readonly
# DESCRIPTION: readonly -p lists read-only variables in re-input form
# EXPECT_EXIT: 0
readonly myvar=v
readonly -p | grep -q '^readonly myvar' || exit 1
