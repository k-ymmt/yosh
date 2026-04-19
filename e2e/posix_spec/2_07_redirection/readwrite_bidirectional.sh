#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file accepts both read and write redirects on the same fd
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_bidir"
echo seed > "$f"
exec 3<>"$f"
exec 3<&-
