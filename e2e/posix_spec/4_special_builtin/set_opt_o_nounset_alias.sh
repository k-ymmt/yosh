#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -o nounset is equivalent to set -u
# EXPECT_EXIT: 1
set -o nounset
unset x
echo "$x"
