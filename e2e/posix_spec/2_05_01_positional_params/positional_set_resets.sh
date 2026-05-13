#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: set -- replaces (not appends) the positional parameters
# EXPECT_OUTPUT: 1:z
# EXPECT_EXIT: 0
set -- x y w
set -- z
echo "$#:$1"
