#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -v echoes each command of a dot script as it is read, but not eval operands
# EXPECT_OUTPUT<<END
# echo a
# a
# echo b
# b
# e
# END
# EXPECT_EXIT: 0
printf 'echo a\necho b\n' > "$TEST_TMPDIR/d.sh"
{ set -v; . "$TEST_TMPDIR/d.sh"; eval 'echo e'; set +v; } 2>&1
