#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a function identifies it as function
# EXPECT_EXIT: 0
myfn() { :; }
type myfn | grep -q function
