#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with input lacking final newline still reads partial line, returns nonzero
# XFAIL: not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: partial
# EXPECT_EXIT: 1
printf 'partial' | { read line; echo "$line"; }
