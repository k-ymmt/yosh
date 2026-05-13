#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias with no args lists all aliases
# EXPECT_EXIT: 0
alias greet='echo hi'
alias | grep -q '^alias greet=' || alias | grep -q '^greet=' || exit 1
