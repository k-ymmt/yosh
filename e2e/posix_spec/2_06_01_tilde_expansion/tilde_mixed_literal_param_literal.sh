#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde after ':' expands even when surrounded by literal and parameter parts
# EXPECT_OUTPUT: /a:/base:/home/x/bin
# EXPECT_EXIT: 0
HOME=/home/x
base=/base
x=/a:$base:~/bin
echo "$x"
