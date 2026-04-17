#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v reports builtin name only
# EXPECT_OUTPUT: cd
# EXPECT_EXIT: 0
command -v cd
