#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Glob * matches files in directory
# EXPECT_EXIT: 0
echo "file1" > "$TEST_TMPDIR/a.txt"
echo "file2" > "$TEST_TMPDIR/b.txt"
cd "$TEST_TMPDIR"
count=0
for f in *.txt; do
  count=$((count + 1))
done
test "$count" = 2
