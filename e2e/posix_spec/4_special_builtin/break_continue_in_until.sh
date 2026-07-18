#!/bin/sh
# POSIX_REF: 2.15 break
# DESCRIPTION: break works in until loops too
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
n=1
until [ "$n" -gt 5 ]; do
    echo $n
    break
done
