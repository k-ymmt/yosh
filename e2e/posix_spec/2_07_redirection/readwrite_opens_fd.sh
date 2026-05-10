#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file opens the file without error
# EXPECT_OUTPUT omitted: this is an open-then-close smoke, not a roundtrip.
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/rw_bidir"
echo seed > "$f"
exec 3<>"$f"
exec 3<&-
