#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>file creates the file if it does not exist
# EXPECT_OUTPUT: created
# EXPECT_EXIT: 0
f="$TEST_TMPDIR/rw_creates"
[ ! -e "$f" ] || { echo "precondition: $f already exists" >&2; exit 1; }
: 1<>"$f"
if [ -e "$f" ]; then
    echo created
else
    echo "not created" >&2
    exit 1
fi
