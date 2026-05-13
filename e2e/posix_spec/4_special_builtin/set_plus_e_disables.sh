#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set +e disables errexit
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
set -e
set +e
false
echo after
