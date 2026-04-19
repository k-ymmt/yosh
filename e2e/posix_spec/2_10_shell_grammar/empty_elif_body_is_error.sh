#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'elif' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then :; elif true; then fi
