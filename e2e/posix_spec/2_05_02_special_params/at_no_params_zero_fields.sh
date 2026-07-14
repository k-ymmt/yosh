#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - @
# DESCRIPTION: "$@" with zero positional parameters expands to zero fields
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
set --
set -- "$@"
echo $#
