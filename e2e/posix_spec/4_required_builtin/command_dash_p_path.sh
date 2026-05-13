#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -p uses the standard PATH for the search
# EXPECT_EXIT: 0
command -p echo found >/dev/null
