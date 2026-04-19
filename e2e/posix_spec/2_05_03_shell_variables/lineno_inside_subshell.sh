#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO inside a subshell reports the enclosed command's line
# EXPECT_OUTPUT: 6
# EXPECT_EXIT: 0
( echo $LINENO )
