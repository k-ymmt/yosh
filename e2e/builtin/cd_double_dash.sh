#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -- treats the following argument as an operand even if it starts with -
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/-foo"
cd "$TEST_TMPDIR"
cd -- -foo
case "$PWD" in
    */-foo) exit 0 ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
