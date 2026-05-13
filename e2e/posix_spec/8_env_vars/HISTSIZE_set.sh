#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTSIZE
# DESCRIPTION: HISTSIZE caps the number of history entries
# EXPECT_EXIT: 0
HISTSIZE=100
[ "$HISTSIZE" = 100 ] || exit 1
