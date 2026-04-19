#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty 'else' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then :; else fi
