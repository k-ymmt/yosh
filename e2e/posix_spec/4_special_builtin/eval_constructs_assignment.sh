#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval re-parses to allow variable name to be computed
# EXPECT_OUTPUT: 42
# EXPECT_EXIT: 0
name=foo
eval "$name=42"
echo "$foo"
