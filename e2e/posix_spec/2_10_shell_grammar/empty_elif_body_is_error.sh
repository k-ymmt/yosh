#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (elif)
# DESCRIPTION: An empty 'elif' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then :; elif true; then fi
