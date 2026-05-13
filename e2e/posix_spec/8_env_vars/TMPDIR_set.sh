#!/bin/sh
# POSIX_REF: 8 Environment Variables - TMPDIR
# DESCRIPTION: TMPDIR is propagated to child processes
# EXPECT_OUTPUT: /custom/tmp
# EXPECT_EXIT: 0
TMPDIR=/custom/tmp
export TMPDIR
sh -c 'echo "$TMPDIR"'
