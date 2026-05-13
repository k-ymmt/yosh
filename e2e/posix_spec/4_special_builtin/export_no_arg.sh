#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with no args lists exported variables (same as -p)
# EXPECT_EXIT: 0
export myvar2=v2
export | grep -q myvar2 || exit 1
