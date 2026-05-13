#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -- ARGS assigns positional parameters
# EXPECT_OUTPUT: one two three
# EXPECT_EXIT: 0
set -- one two three
echo "$1 $2 $3"
