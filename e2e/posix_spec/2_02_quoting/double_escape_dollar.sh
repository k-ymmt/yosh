#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash escapes $ inside double-quotes
# EXPECT_OUTPUT: $x
# EXPECT_EXIT: 0
x=value
echo "\$x"
