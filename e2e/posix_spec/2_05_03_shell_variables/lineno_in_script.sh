#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO expands to the current script line number
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
# XFAIL: LINENO is not expanded (yields empty string) in yosh
echo $LINENO
