#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: noclobber prevents overwriting, >| overrides
# EXPECT_OUTPUT<<END
# original
# override
# END
echo original > "$TEST_TMPDIR/file.txt"
set -C
echo new > "$TEST_TMPDIR/file.txt" 2>/dev/null
cat "$TEST_TMPDIR/file.txt"
echo override >| "$TEST_TMPDIR/file.txt"
cat "$TEST_TMPDIR/file.txt"
