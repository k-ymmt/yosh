#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: Positional parameters $1 through $3 via set --
# EXPECT_OUTPUT: a b c
set -- a b c
echo "$1 $2 $3"
