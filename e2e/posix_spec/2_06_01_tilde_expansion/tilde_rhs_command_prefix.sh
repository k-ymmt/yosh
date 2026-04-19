#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde in a command-prefix assignment expands before the command runs
# EXPECT_OUTPUT: /home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
PREFIXED=~/bin sh -c 'echo "$PREFIXED"'
