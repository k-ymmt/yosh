#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N where N is unquoted parameter expansion still duplicates
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/dup_in_unquoted"
echo hello > "$f"
exec 3< "$f"
fd=3
cat <&$fd
exec 3<&-
