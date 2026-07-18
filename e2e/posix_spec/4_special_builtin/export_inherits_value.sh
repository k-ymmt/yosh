#!/bin/sh
# POSIX_REF: 2.15 export
# DESCRIPTION: export of an existing variable keeps its current value
# EXPECT_OUTPUT: keep
# EXPECT_EXIT: 0
foo=keep
export foo
sh -c 'echo "$foo"'
