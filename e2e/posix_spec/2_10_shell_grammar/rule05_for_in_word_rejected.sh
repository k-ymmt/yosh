#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: `in` is a reserved word and must be rejected as NAME
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
for in in a; do
    :
done
