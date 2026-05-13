#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias is not re-expanded on itself
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
alias ls='echo x'
ls
