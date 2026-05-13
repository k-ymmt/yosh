#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset with no operands is not an error
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
unset
echo $?
