#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: $'\cX' produces the corresponding control character
# EXPECT_OUTPUT: ctrl-ok
# EXPECT_EXIT: 0
v=$'\cA'
[ "$v" = "$(printf '\001')" ] && echo ctrl-ok
