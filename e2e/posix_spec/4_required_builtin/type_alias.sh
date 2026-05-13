#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on an alias identifies it as alias
# XFAIL: non-POSIX deviation (yosh has no native type builtin; falls through to /usr/bin/type which cannot see yosh's session aliases)
# EXPECT_EXIT: 0
alias myalias='echo x'
type myalias | grep -q alias
