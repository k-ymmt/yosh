#!/bin/sh
# POSIX_REF: 2.10.2 Rule 8 - NAME in function
# DESCRIPTION: A reserved word is not a valid function name per POSIX
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
if() { :; }
