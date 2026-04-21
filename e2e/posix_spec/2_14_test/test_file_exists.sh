#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -e is true for an existing file
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
f="$TEST_TMPDIR/file_exists_$$"
: > "$f"
[ -e "$f" ] && echo yes
rm -f "$f"
