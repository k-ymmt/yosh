#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip shortest prefix with ${var#pattern}
# EXPECT_OUTPUT: path/to/file.txt
f=/path/to/file.txt
echo "${f#/}"
