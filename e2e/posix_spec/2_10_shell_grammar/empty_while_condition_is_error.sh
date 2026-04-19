#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (while cond)
# DESCRIPTION: An empty 'while' condition is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
while do done
