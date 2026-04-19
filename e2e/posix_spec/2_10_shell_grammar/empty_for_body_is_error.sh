#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (for body)
# DESCRIPTION: An empty 'for' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
for i in a; do done
