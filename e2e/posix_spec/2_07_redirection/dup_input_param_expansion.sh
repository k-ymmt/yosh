#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&"$fd" accepts an fd number via parameter expansion
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_in_pe"
echo hi > "$f"
exec 3< "$f"
fd=3
cat <&"$fd"
exec 3<&-
