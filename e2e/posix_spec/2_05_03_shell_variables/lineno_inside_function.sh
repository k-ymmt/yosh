#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO inside a function body reports the body command's line
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
f() {
    echo $LINENO
}
f
