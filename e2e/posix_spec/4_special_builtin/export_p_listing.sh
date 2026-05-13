#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export -p lists exported variables in re-input form
# EXPECT_EXIT: 0
export myvar=value
export -p | grep -q '^export myvar' || exit 1
