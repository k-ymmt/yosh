#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A name cannot start with a digit
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for 1x in a; do
    :
done
