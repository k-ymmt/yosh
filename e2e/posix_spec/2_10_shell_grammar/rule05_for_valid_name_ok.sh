#!/bin/sh
# POSIX_REF: 2.10.2 Rule 5 - NAME in for
# DESCRIPTION: A plain identifier NAME is accepted
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
# EXPECT_EXIT: 0
for i in a b c; do
    echo $i
done
