#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG sets default locale category values
# XFAIL: not yet implemented (TODO: implement locale handling; yosh has no locale support yet)
# EXPECT_EXIT: 0
LANG=C
[ "$(echo b a | sort | head -n1)" = a ] || exit 1
