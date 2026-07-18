#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -e ignores the failure of a non-final AND-OR list component
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
set -e
false && true
echo after
