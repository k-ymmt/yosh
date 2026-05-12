#!/bin/sh
# POSIX_REF: 2.14.14 trap
# DESCRIPTION: trap '' SIGNAL ignores the signal
# EXPECT_EXIT: 0
trap '' USR1
kill -USR1 $$
echo "survived"
