#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -u treats expansion of an unset positional parameter as an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: parameter not set
set -u
echo "[$1]"
