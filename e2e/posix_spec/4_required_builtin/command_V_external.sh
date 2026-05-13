#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -V prints a human-readable identification
# EXPECT_EXIT: 0
command -V echo >/dev/null
