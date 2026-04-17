#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command bypasses user-defined functions
# EXPECT_OUTPUT: real
# EXPECT_EXIT: 0
printf() { echo "fake"; }
command printf "real"
