#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Modulo operator returns remainder
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
echo $((10%3))
