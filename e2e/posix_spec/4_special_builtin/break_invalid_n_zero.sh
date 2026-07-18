#!/bin/sh
# POSIX_REF: 2.15 break
# DESCRIPTION: break 0 is an invalid operand
# EXPECT_EXIT: 2
# EXPECT_STDERR: break
for i in 1 2; do
    break 0
done
