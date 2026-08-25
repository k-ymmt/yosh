#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap '' on a non-default-handled signal installs SIG_IGN
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
trap '' ABRT
kill -ABRT $$
echo after
