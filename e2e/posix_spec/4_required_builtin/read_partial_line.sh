#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with input lacking final newline still reads partial line, returns nonzero
# EXPECT_OUTPUT: partial
# EXPECT_EXIT: 1
printf 'partial' | { read line; rc=$?; echo "$line"; exit $rc; }
