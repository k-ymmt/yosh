#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N where N is unquoted parameter expansion still duplicates
# EXPECT_OUTPUT: file:hello
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/dup_out_unquoted"
exec 3> "$f"
fd=3
echo hello >&$fd
exec 3>&-
# 'file:' marker forces fail if >&3 silently became a no-op (see spec)
printf 'file:'
cat "$f"
