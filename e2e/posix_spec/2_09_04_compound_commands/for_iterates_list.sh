#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: for iterates over a word list
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for i in a b; do
    echo "$i"
done
