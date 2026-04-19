#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'while' condition is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
while do done
