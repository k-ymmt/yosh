#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with multiple NAME=value pairs exports all
# EXPECT_OUTPUT: 1-2
# EXPECT_EXIT: 0
export a=1 b=2
sh -c 'echo "$a-$b"'
