#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'for' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
for i in a; do done
