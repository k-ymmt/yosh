#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Unset variable is treated as zero in arithmetic context
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
unset x
echo $((x+1))
