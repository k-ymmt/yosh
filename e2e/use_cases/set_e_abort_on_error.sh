#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: Use case - abort a script on the first failing command with set -e
# EXPECT_OUTPUT: step 1 ok
# EXPECT_EXIT: 1
set -e
echo "step 1 ok"
false
echo "step 2 never runs"
