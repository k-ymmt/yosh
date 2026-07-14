#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: ${01} is interpreted as a decimal number, equivalent to $1
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
# XFAIL: yosh rejects leading zeros in positional parameter expansion
set -- a b
echo "${01}"
