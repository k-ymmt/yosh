#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: A command substitution in PS4 is not itself traced (no recursion)
# EXPECT_OUTPUT<<END
# X> echo a
# a
# END
# EXPECT_EXIT: 0
out=$( (PS4='$(echo X)> '; set -x; echo a) 2>&1 )
printf '%s\n' "$out"
