#!/bin/sh
# POSIX_REF: 2.14.18 unset (XBD 12.2 Guideline 10)
# DESCRIPTION: unset honors -- after flag parsing
# EXPECT_OUTPUT: empty
# EXPECT_EXIT: 0
m=set
unset -- m || exit 99
echo "${m-empty}"
