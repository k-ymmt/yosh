#!/bin/sh
# POSIX_REF: 2.15 export
# DESCRIPTION: An exported-but-unset name is still unset for set -u
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: y
export y
set -u
echo "$y"
echo after
