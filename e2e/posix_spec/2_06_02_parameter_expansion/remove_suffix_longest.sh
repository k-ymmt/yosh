#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var%%pattern} removes longest matching suffix
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
x=a.b.c
echo "${x%%.*}"
