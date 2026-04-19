#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_command
# DESCRIPTION: An empty subshell is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
( )
