#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: "$*" expands to all positional parameters as single field
# EXPECT_OUTPUT: a b c
set -- a b c
echo "$*"
