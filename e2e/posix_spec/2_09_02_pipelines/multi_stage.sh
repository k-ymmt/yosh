#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Multi-stage pipeline passes data through each stage in order
# EXPECT_OUTPUT: c
# EXPECT_EXIT: 0
echo a | tr a b | tr b c
