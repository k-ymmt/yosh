#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands when ':' and '~' are separated by line-continuation (POSIX §2.2.1 removes \<newline> before tokenization)
# EXPECT_OUTPUT: foo:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
x=foo:\
~/bin
echo "$x"
