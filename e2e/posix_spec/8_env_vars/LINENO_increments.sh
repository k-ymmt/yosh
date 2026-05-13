#!/bin/sh
# POSIX_REF: 8 Environment Variables - LINENO
# DESCRIPTION: LINENO changes between successive lines
# EXPECT_EXIT: 0
a="$LINENO"
b="$LINENO"
[ "$a" != "$b" ] || exit 1
