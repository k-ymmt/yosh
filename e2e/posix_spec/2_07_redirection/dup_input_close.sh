#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&- closes an input fd; subsequent <&N on the same fd fails
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
f="$TEST_TMPDIR/dup_in_close"
echo gone > "$f"
exec 3< "$f"
exec 3<&-
cat <&3
