#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${r:=word} on a readonly null variable is an expansion error that aborts the script
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
readonly r=
echo before
echo ${r:=2}
echo after
