#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A valid NAME after 'for' is accepted
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for x in a b; do
    echo "$x"
done
