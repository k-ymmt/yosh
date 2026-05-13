#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on an alias identifies it as alias
# XFAIL: type does not report aliases defined in the same session (via /usr/bin/type wrapper)
# EXPECT_EXIT: 0
alias myalias='echo x'
type myalias | grep -q alias
