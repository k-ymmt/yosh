#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Non-ASCII text around arithmetic expansion is not corrupted
# EXPECT_OUTPUT: 日本 2 語
echo 日本 $(( 1 + 1 )) 語
