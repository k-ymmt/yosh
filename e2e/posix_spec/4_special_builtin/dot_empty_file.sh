#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: Sourcing an empty file yields exit status 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
: > "$TEST_TMPDIR/empty.sh"
. "$TEST_TMPDIR/empty.sh"
echo $?
