#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: arguments after an alias invocation are passed through
# EXPECT_OUTPUT: from-args
# EXPECT_EXIT: 0
alias say='echo'
say from-args
