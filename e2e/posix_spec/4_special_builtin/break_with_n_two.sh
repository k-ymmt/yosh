#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break 2 exits two enclosing loops
# EXPECT_OUTPUT: outer1-inner1
# EXPECT_EXIT: 0
for i in 1 2; do
    for j in 1 2; do
        echo outer$i-inner$j
        break 2
    done
done
