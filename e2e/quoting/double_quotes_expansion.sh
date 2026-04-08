#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Double quotes allow command and arithmetic expansion
# EXPECT_OUTPUT: cmd=hello arith=3
echo "cmd=$(echo hello) arith=$((1+2))"
