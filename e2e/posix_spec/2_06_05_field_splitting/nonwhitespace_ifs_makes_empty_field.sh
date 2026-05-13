#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Adjacent non-whitespace IFS characters yield empty fields
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS=:
x=a::b
set -- $x
echo $#
