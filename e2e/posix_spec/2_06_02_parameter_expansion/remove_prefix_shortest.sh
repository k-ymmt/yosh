#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var#pattern} removes shortest matching prefix
# EXPECT_OUTPUT: to/file
# EXPECT_EXIT: 0
x=path/to/file
echo "${x#path/}"
