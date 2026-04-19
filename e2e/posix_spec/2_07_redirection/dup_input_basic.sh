#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N duplicates input fd N to fd 0 for the command
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/dup_in"
echo hello > "$f"
exec 3< "$f"
cat <&3
exec 3<&-
