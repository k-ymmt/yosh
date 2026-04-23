#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&"$fd" accepts an fd number via parameter expansion
# EXPECT_OUTPUT: file:hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_out_pe"
exec 3> "$f"
fd=3
echo hi >&"$fd"
exec 3>&-
# 'file:' marker forces fail if >&"$fd" silently became a no-op (see spec)
printf 'file:'
cat "$f"
