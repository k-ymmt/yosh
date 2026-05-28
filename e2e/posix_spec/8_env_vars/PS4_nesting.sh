#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 first character repeats by function-nesting level
# EXPECT_STDERR: ++ echo deep
# EXPECT_OUTPUT: deep
# EXPECT_EXIT: 0
PS4='+ '
f() { echo deep; }
set -x
f
