#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: A command substitution in PS4 does not change $? seen by the traced command
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
PS4='$(true)+ '
set -x
false
echo $? 2>/dev/null
