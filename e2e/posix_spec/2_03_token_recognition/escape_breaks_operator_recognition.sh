#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Backslash-escaped operator character is treated as a word character
# EXPECT_OUTPUT: &
# EXPECT_EXIT: 0
echo \&
