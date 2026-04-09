#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline commands run in subshells — variable changes do not propagate
# EXPECT_OUTPUT: before
x=before
echo test | x=after
echo "$x"
