#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias with empty value defines an alias that runs nothing
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
alias noop=''
noop
