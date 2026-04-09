#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap '' SIGNAL ignores the signal
# EXPECT_EXIT: 0
trap '' USR1
kill -USR1 $$
echo "survived"
