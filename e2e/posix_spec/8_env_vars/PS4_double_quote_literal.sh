#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: A double quote in PS4 is an ordinary character (here-document-like expansion)
# EXPECT_OUTPUT<<END
# say "hi" + echo a
# a
# a\"bC + echo b
# b
# END
# EXPECT_EXIT: 0
c=C
out=$( (PS4='say "hi" + '; set -x; echo a) 2>&1 )
printf '%s\n' "$out"
out=$( (PS4='a\"b$c + '; set -x; echo b) 2>&1 )
printf '%s\n' "$out"
