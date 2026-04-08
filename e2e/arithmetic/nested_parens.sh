#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Nested parentheses for grouping
# EXPECT_OUTPUT: 20
echo $(( (2 + 3) * 4 ))
