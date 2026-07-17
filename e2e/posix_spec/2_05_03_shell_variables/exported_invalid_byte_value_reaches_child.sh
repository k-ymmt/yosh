#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: Exported variable value with an invalid UTF-8 byte reaches a child process byte-identically
# EXPECT_OUTPUT: e9
# EXPECT_EXIT: 0
FOO=$'\xe9'
export FOO
/bin/sh -c 'printf %s "$FOO"' | od -An -tx1 | tr -d ' \n'
echo
