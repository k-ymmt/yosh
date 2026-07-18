#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -x traces each assignment on its own line (matches bash/dash)
# EXPECT_STDERR: + b=2
# EXPECT_EXIT: 0
set -x
a=1 b=2
