#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: : (colon) always returns exit code 0
# EXPECT_OUTPUT: 0
:
echo "$?"
