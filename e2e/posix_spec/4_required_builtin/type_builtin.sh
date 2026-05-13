#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a builtin identifies it as such
# EXPECT_EXIT: 0
type cd | grep -q builtin
