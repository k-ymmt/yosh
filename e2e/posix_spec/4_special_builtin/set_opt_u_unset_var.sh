#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -u treats expansion of unset variable as an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: parameter not set
set -u
unset x
echo "$x"
