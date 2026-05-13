#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly -p lists read-only variables in re-input form
# XFAIL: non-POSIX deviation (yosh readonly -p produces no output; use readonly without -p instead)
# EXPECT_EXIT: 0
readonly myvar=v
readonly -p | grep -q '^readonly myvar' || exit 1
