#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap '' SIGNAL ignores the signal
# EXPECT_OUTPUT: still alive
# EXPECT_EXIT: 0
trap '' TERM
kill -TERM $$
echo still alive
