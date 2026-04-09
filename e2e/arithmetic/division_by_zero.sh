#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Division by zero produces error
# EXPECT_EXIT: 1
# EXPECT_STDERR: division by zero
echo $((1 / 0))
