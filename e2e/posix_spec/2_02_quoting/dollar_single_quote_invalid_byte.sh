#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: \xHH escape denotes a raw byte even when not valid UTF-8
# EXPECT_OUTPUT: e9
# EXPECT_EXIT: 0
printf '%s' $'\xe9' | od -An -tx1 | tr -d ' \n'
echo
