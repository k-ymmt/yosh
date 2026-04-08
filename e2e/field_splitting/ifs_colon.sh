#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: IFS with non-whitespace delimiter
# EXPECT_OUTPUT<<END
# a
# b
# END
IFS=:
x="a:b"
set -- $x
echo "$1"
echo "$2"
