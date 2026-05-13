#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export -- treats following operands as names
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
export -- foo=hi
sh -c 'echo "$foo"'
