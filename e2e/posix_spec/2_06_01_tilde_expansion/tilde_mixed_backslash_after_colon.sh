#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: backslash-tilde after colon stays literal (no tilde expansion)
# EXPECT_OUTPUT: foo:~/bin
# EXPECT_EXIT: 0
x=foo:\~/bin
echo "$x"
