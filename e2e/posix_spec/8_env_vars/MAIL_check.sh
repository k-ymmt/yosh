#!/bin/sh
# POSIX_REF: 8 Environment Variables - MAIL
# DESCRIPTION: MAIL names a single mailbox file the shell checks before each prompt
# EXPECT_EXIT: 0
MAIL="$TEST_TMPDIR/inbox"
exit 0
