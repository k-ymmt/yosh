#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e exits when the LAST command of a pipeline fails
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
set -e
echo before
true | false
echo unreached
