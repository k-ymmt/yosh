#!/bin/sh
# POSIX_REF: 2.15 readonly
# DESCRIPTION: readonly NAME= sets an empty read-only string
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
readonly foo=
echo "<$foo>"
