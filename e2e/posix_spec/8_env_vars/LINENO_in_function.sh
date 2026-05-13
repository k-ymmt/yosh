#!/bin/sh
# POSIX_REF: 8 Environment Variables - LINENO
# DESCRIPTION: LINENO works inside a function
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
f() { echo "$LINENO"; }
# this line is line 5 from the file start (counting the header comments)
f
