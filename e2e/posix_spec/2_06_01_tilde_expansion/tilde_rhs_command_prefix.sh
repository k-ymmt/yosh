#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde in a command-prefix assignment expands before the command runs
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
# Verify command-prefix assignment with tilde expansion by invoking a child
# yosh directly ($0 is the script path per POSIX §2.1, so it can no longer
# be used to re-invoke the shell under test).
HOME=/home/x
PREFIXED=~/bin ./target/debug/yosh -c 'echo "$PREFIXED"'
