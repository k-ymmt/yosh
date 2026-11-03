#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: A variable whose value is an invalid octal constant (09) is an arithmetic error
# EXPECT_OUTPUT: before
# EXPECT_EXIT: 1
x=09
echo before
echo $((x))
echo after
