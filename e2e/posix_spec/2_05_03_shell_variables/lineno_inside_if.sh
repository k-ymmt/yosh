#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO inside a then-body reports the echo's line
# EXPECT_OUTPUT: 8
# EXPECT_EXIT: 0
if true
then
    echo $LINENO
fi
