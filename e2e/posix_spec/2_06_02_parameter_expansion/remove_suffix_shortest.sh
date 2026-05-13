#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var%pattern} removes shortest matching suffix
# EXPECT_OUTPUT: foo
# EXPECT_EXIT: 0
x=foo.txt
echo "${x%.txt}"
