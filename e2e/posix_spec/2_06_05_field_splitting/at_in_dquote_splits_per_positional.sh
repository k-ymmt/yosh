#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: "$@" expands to separate fields per positional parameter
# EXPECT_OUTPUT<<END
# [a b]
# [c]
# END
# EXPECT_EXIT: 0
set -- "a b" c
for w in "$@"; do
    echo "[$w]"
done
