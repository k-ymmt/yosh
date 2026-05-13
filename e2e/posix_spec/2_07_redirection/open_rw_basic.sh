#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: <> opens a file for reading and writing
# EXPECT_OUTPUT: data
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
exec 3<>f
echo data >&3
exec 3<&-
cat f
