#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: `for in in in` uses `in` as the loop NAME (dash, bash, ksh agree)
# EXPECT_OUTPUT: in
# EXPECT_EXIT: 0
for in in in; do echo $in; done
