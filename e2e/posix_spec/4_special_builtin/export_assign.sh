#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export NAME=value assigns and exports atomically
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
export foo=hello
sh -c 'echo "$foo"'
