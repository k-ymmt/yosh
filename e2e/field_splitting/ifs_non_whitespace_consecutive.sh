#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Consecutive non-whitespace IFS delimiters produce empty fields
# EXPECT_EXIT: 0
IFS=:
x="a::b"
set -- $x
test "$#" = 3 && test "$1" = "a" && test "$2" = "" && test "$3" = "b"
