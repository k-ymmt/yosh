#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (else)
# DESCRIPTION: An empty 'else' body is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then :; else fi
