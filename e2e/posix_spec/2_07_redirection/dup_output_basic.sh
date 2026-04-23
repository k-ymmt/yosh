#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N duplicates output fd N to fd 1 for the command
# EXPECT_OUTPUT: file:hello
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_out"
exec 3> "$f"
echo hello >&3
exec 3>&-
# 'file:' marker forces fail if >&3 silently became a no-op (see spec)
printf 'file:'
cat "$f"
