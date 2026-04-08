#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip shortest suffix with ${var%pattern}
# EXPECT_OUTPUT: /path/to/file
f=/path/to/file.txt
echo "${f%.txt}"
