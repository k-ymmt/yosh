#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -C (noclobber) prevents > redirection from overwriting existing files
# EXPECT_EXIT: 1
echo first > "$TEST_TMPDIR/f"
set -C
echo second > "$TEST_TMPDIR/f"
