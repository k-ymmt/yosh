#!/bin/sh
# POSIX_REF: 8 Environment Variables - TMPDIR
# DESCRIPTION: TMPDIR is honored by the shell when creating temp files (here-doc, etc.)
# EXPECT_EXIT: 0
TMPDIR="$TEST_TMPDIR"
exit 0
