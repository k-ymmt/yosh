#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: A=1 cmd sets A for cmd only (transient)
# EXPECT_EXIT: 0
# EXPECT_OUTPUT: 1
A=1 env | grep '^A=1' >/dev/null || { echo "transient A not in env" >&2; exit 1; }
echo 1
