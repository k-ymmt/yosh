#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Parameter expansion occurs inside double-quotes
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
echo "$x"
