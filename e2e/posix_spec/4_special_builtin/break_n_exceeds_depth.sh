#!/bin/sh
# POSIX_REF: 2.15 break
# DESCRIPTION: break with n exceeding loop nesting exits outermost loop (per POSIX, no error)
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
for i in a b; do
    echo $i
    break 5
done
