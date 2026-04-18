#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd with CDPATH finds the operand under a CDPATH entry and prints the new PWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/sub"
CDPATH="$TEST_TMPDIR" cd sub > "$TEST_TMPDIR/out"
case "$PWD" in
    *"/sub") ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
grep -q sub "$TEST_TMPDIR/out" || { echo "stdout missing sub"; exit 1; }
