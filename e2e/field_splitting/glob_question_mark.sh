#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: ? matches exactly one character
# EXPECT_EXIT: 0
echo "x" > "$TEST_TMPDIR/a1.txt"
echo "y" > "$TEST_TMPDIR/b2.txt"
echo "z" > "$TEST_TMPDIR/cc.txt"
cd "$TEST_TMPDIR"
count=0
for f in ??.txt; do
  count=$((count + 1))
done
test "$count" = 3
