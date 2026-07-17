#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment-only command with a pure arithmetic expansion yields $?=0 (bash/dash behavior)
# EXPECT_OUTPUT: 0
false
x=$((1+1))
echo $?
