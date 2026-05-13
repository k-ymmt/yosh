#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read -r preserves backslashes in input
# XFAIL: not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: a\b
# EXPECT_EXIT: 0
printf 'a\\b\n' | { read -r line; echo "$line"; }
