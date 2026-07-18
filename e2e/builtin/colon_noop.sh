#!/bin/sh
# POSIX_REF: 2.15 colon
# DESCRIPTION: : (colon) is a no-op that returns 0
# EXPECT_OUTPUT: 0
:
echo "$?"
