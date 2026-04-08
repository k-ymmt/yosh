#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $# holds count of positional parameters
# EXPECT_OUTPUT: 3
set -- a b c
echo "$#"
