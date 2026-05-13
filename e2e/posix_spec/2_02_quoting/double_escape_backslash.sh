#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash escapes itself inside double-quotes
# EXPECT_OUTPUT: \
# EXPECT_EXIT: 0
echo "\\"
