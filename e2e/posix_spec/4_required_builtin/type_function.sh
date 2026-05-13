#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a function identifies it as function
# XFAIL: type does not report functions defined in the same session (via /usr/bin/type wrapper)
# EXPECT_EXIT: 0
myfn() { :; }
type myfn | grep -q function
