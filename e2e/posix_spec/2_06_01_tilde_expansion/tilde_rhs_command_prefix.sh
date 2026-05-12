#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde in a command-prefix assignment expands before the command runs
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
# Verify command-prefix assignment with tilde expansion by invoking a child
# yosh ($0 is the shell running this script per e2e/run_tests.sh) instead of
# an external sh -c, so the test is hermetic on minimal environments.
HOME=/home/x
PREFIXED=~/bin "$0" -c 'echo "$PREFIXED"'
