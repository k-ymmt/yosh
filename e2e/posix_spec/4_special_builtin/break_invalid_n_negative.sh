#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break -1 is an invalid operand
# EXPECT_EXIT: 2
# EXPECT_STDERR: break
for i in 1 2; do
    break -1
done
