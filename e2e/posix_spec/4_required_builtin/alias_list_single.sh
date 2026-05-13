#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias NAME prints the named alias
# EXPECT_EXIT: 0
alias greet='echo hi'
alias greet | grep -q "greet=" || exit 1
