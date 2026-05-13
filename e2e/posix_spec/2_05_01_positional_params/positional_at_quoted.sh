#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: "$@" expands to separate words preserving whitespace
# EXPECT_OUTPUT<<END
# a b
# c
# END
# EXPECT_EXIT: 0
set -- "a b" c
for w in "$@"; do
    echo "$w"
done
