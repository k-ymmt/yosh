#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e still exits when the final command of an AND-OR list fails
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
set -e
true && false
echo not-reached
