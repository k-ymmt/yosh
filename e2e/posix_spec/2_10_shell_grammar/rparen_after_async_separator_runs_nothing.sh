#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: A ')' after '&' at top level is a syntax error for the whole line; no command on it runs
# EXPECT_OUTPUT:
# EXPECT_EXIT: 2
# EXPECT_STDERR: )
echo first & echo second; )
