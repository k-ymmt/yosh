#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec of a non-executable file exits 126
# EXPECT_EXIT: 126
: > "$TEST_TMPDIR/notexec"
chmod 644 "$TEST_TMPDIR/notexec"
exec "$TEST_TMPDIR/notexec" 2>/dev/null
