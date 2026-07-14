#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: # begins a comment only at the start of a token
# EXPECT_OUTPUT: a#b
# EXPECT_EXIT: 0
echo a#b
