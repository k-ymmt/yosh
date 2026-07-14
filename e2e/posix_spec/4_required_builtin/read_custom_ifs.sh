#!/bin/sh
# POSIX_REF: read utility
# DESCRIPTION: read splits input using the current IFS
# EXPECT_OUTPUT: a|b|c
# EXPECT_EXIT: 0
printf 'a:b:c\n' > "$TEST_TMPDIR/in"
IFS=: read x y z < "$TEST_TMPDIR/in"
echo "$x|$y|$z"
