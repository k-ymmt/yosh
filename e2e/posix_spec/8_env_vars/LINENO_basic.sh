#!/bin/sh
# POSIX_REF: 8 Environment Variables - LINENO
# DESCRIPTION: LINENO expands to the current line number
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
# (line numbers count from 1; this echo is on line 5)
echo "$LINENO"
