#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: $'\xHH' hexadecimal and $'\ddd' octal escapes produce the corresponding bytes
# EXPECT_OUTPUT: AB
# EXPECT_EXIT: 0
v=$'\x41\102'
echo "$v"
