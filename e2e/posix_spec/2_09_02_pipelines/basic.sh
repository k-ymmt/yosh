#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: | pipes stdout of left to stdin of right
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
echo a | cat
