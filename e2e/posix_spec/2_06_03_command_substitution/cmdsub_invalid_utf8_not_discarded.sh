#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Output containing an invalid UTF-8 byte is captured, not discarded
# EXPECT_OUTPUT: kept
# EXPECT_EXIT: 0
v=$(printf 'a\377b')
[ -n "$v" ] && echo kept
