#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: The word after `for` is not in a reserved-word position (2.4), so `if` is a valid NAME
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for if in a b; do
    echo "$if"
done
