#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: trap '' SIGNAL ignores the signal
# EXPECT_EXIT: 0
# XFAIL: USR1 signal kills process even with trap '' (shell limitation)
trap '' USR1
kill -USR1 $$
echo "survived"
