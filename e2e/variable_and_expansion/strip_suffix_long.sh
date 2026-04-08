#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Strip longest suffix with ${var%%pattern}
# EXPECT_OUTPUT: /path/to/file
f=/path/to/file.tar.gz
echo "${f%%.*}"
