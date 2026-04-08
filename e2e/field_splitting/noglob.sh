#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: set -f disables pathname expansion
# EXPECT_OUTPUT: *
set -f
echo *
