#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: \c\\ is control-backslash, and a NUL escape ends the string so it can be passed to a utility
# EXPECT_OUTPUT<<END
# 1c78
# a|a|a
# END
# EXPECT_EXIT: 0
printf %s $'\c\\x' | od -An -tx1 | tr -d ' \n'
echo
printf '%s|' $'a\0b' $'a\x00b'
/bin/echo $'a\000c'
