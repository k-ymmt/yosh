#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on a special builtin prints the name
# EXPECT_OUTPUT: export
# EXPECT_EXIT: 0
command -v export
