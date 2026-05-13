#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Adjacent whitespace IFS characters collapse to one delimiter
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
x="a    b"
set -- $x
echo $#
