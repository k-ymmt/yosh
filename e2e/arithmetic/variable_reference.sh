#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Variables referenced in arithmetic without $ prefix
# EXPECT_OUTPUT: 13
x=10
y=3
echo $((x + y))
