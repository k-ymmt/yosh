#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -v echoes an input line holding a byte that is not valid UTF-8 with its original bytes
# EXPECT_OUTPUT: 3a 20 ff 0a
# EXPECT_EXIT: 0
printf 'set -v\n: \377\n' > "$TEST_TMPDIR/v.sh"
./target/debug/yosh "$TEST_TMPDIR/v.sh" 2>"$TEST_TMPDIR/err" >/dev/null
tail -n 1 "$TEST_TMPDIR/err" | od -An -tx1 | tr -s ' ' | sed 's/^ //; s/ $//'
