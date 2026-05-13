#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG sets default locale category values
# XFAIL: locale support not implemented in yosh (TODO: implement locale handling)
# EXPECT_EXIT: 0
LANG=C
[ "$(echo b a | sort | head -n1)" = a ] || exit 1
