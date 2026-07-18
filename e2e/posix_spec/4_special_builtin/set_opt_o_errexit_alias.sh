#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -o errexit is equivalent to set -e
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
set -o errexit
echo before
false
echo after
