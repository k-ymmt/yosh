#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip longest prefix with ${var##pattern}
# EXPECT_OUTPUT: file.txt
f=/path/to/file.txt
echo "${f##*/}"
