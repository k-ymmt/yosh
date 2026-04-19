#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file opens file for read+write on fd N; written data is readable afterwards
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_basic"
echo hi 1<>"$f"
cat "$f"
