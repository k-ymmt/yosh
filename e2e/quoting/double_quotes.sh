#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Double quotes allow variable expansion
# EXPECT_OUTPUT: value is hello
x=hello
echo "value is $x"
