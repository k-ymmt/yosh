#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (until cond)
# DESCRIPTION: An empty 'until' condition is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
until do done
