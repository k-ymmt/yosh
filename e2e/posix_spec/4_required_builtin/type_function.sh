#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a function identifies it as function
# XFAIL: non-POSIX deviation (yosh has no native type builtin; falls through to /usr/bin/type which cannot see yosh's session functions)
# EXPECT_EXIT: 0
myfn() { :; }
type myfn | grep -q function
