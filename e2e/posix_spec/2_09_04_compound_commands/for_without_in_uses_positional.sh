#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: for without "in" iterates over positional parameters
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
set -- a b
for i; do
    echo "$i"
done
