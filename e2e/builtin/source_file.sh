#!/bin/sh
# POSIX_REF: 2.14.4 dot
# DESCRIPTION: . (dot) sources a file in the current environment
# EXPECT_OUTPUT: sourced
echo 'MY_SRC_VAR=sourced' > "$TEST_TMPDIR/lib.sh"
. "$TEST_TMPDIR/lib.sh"
echo "$MY_SRC_VAR"
