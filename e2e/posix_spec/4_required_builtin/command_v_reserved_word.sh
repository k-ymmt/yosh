#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on a reserved word reports the word itself
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
command -v if
