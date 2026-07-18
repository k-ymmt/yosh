#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: test = compares invalid UTF-8 bytes byte-exactly across sources (cmd-sub vs $'...')
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
v=$(printf 'a\351b')
[ "$v" = $'a\xe9b' ] && [ "$v" != $'a\xffb' ] && echo ok
