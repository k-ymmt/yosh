#!/bin/sh
# POSIX_REF: 8 Environment Variables - MAILPATH
# DESCRIPTION: MAILPATH is a colon-separated list of mailboxes, each optionally followed by ?message
# EXPECT_EXIT: 0
MAILPATH="$TEST_TMPDIR/a:$TEST_TMPDIR/b"
exit 0
