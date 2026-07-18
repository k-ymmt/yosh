#!/bin/sh
# POSIX_REF: 2.15 break
# DESCRIPTION: break with no operand exits the innermost enclosing loop
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
for i in 1 2 3; do
    echo $i
    break
done
