#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG sets default locale category values; LANG=C → C collation
# EXPECT_EXIT: 0
LANG=C
[ "$(printf '%s\n' b a | sort | head -n1)" = a ] || exit 1
