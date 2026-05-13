#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias of an undefined name is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: alias
alias nosuch
