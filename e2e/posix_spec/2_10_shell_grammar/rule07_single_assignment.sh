#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: NAME=value at word position is a persistent assignment
# EXPECT_OUTPUT: bar
# EXPECT_EXIT: 0
FOO=bar
echo "$FOO"
