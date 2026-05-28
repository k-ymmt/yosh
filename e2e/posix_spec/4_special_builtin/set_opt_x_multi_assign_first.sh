#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces the first assignment of a multi-assignment command (sibling to set_opt_x_multi_assign.sh)
# EXPECT_STDERR: + a=1
# EXPECT_EXIT: 0
set -x
a=1 b=2
