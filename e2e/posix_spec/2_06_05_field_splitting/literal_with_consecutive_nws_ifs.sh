#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Multiple adjacent non-whitespace IFS chars in a literal stay intact (no empty fields)
# EXPECT_OUTPUT: [a:b:c]
# EXPECT_EXIT: 0
IFS=:
printf "[%s]\n" a:b:c
