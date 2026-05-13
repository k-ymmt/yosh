#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e causes shell to exit on simple command failure
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
set -e
echo before
false
echo after
