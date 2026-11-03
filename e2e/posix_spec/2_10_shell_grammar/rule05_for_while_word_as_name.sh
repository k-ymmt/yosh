#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: `while` after `for` is not in a reserved-word position, so it is a valid NAME
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
for while in a; do
    echo "$while"
done
