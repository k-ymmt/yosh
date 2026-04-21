#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -f is true for a regular file, false for a directory
# EXPECT_OUTPUT: regular notdir
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/regular_$$"
: > "$f"
[ -f "$f" ] && printf 'regular '
[ -f "$TEST_TMPDIR" ] || printf 'notdir'
echo
rm -f "$f"
