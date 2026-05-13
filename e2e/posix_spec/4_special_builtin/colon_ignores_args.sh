#!/bin/sh
# POSIX_REF: 2.14.4 colon
# DESCRIPTION: colon ignores all positional args and returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
: one two three
echo $?
