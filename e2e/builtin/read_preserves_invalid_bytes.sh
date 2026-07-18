#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read preserves invalid UTF-8 bytes instead of replacing them
# EXPECT_OUTPUT: 61e962
# EXPECT_EXIT: 0
printf 'a\351b\n' | {
    read x
    printf '%s' "$x" | od -An -tx1 | tr -d ' \n'
    echo
}
