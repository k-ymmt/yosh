#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: An empty CDPATH entry (leading colon) means current directory
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/sub"
cd "$TEST_TMPDIR"
CDPATH=":/nonexistent" cd sub
case "$PWD" in
    *"/sub") exit 0 ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
