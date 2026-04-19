#!/bin/sh
# POSIX_REF: 2.10.2 Rule 2 - Redirection filename
# DESCRIPTION: A reserved word is treated as a plain filename in a redirection target
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo hi > "$TEST_TMPDIR/if"
cat "$TEST_TMPDIR/if"
