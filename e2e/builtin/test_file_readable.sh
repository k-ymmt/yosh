#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: -r reflects read permission (via chmod)
# EXPECT_OUTPUT: readable
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/readable_$$"
: > "$f"
chmod 0644 "$f"
[ -r "$f" ] && echo readable
rm -f "$f"
