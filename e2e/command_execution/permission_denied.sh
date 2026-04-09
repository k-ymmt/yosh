#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Non-executable file exits with 126
# EXPECT_EXIT: 126
echo "not executable" > "$TEST_TMPDIR/noperm.sh"
chmod -x "$TEST_TMPDIR/noperm.sh"
"$TEST_TMPDIR/noperm.sh"
