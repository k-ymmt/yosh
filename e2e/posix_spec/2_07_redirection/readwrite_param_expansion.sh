#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>"$file" accepts a filename via parameter expansion
# EXPECT_OUTPUT: roundtrip
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_pe"
echo roundtrip 1<>"$f"
cat "$f"
