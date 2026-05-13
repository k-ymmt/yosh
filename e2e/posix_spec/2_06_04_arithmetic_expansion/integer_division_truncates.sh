#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Integer division truncates toward zero
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
echo $((10/3))
