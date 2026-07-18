#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -x traces a single assignment-only command as + name=value
# EXPECT_STDERR: + x=1
# EXPECT_EXIT: 0
set -x
x=1
