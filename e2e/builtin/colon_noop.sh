#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: : (colon) is a no-op that returns 0
# EXPECT_OUTPUT: 0
:
echo "$?"
