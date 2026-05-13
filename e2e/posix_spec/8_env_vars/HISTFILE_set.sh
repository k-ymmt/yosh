#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTFILE
# DESCRIPTION: HISTFILE names the file used to save command history
# EXPECT_EXIT: 0
HISTFILE="$TEST_TMPDIR/history"
[ "$HISTFILE" = "$TEST_TMPDIR/history" ] || exit 1
