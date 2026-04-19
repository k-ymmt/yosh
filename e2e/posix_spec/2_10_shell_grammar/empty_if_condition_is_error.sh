#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'if' condition (before 'then') is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if then true; fi
