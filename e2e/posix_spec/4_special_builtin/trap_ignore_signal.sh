#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap '' SIGNAL causes the signal to be ignored
# EXPECT_OUTPUT: survived
# EXPECT_EXIT: 0
trap '' TERM
kill -TERM $$ 2>/dev/null
echo survived
