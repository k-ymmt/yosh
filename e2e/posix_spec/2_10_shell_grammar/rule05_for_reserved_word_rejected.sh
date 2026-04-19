#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A reserved word is not a valid NAME
# XFAIL: yosh accepts reserved words as for-loop NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for if in a; do
    :
done
