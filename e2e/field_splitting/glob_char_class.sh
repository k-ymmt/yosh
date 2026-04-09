#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Character class glob patterns [a-c]
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo x > a1.txt
echo x > b2.txt
echo x > c3.log
count=0
for f in [a-c]*.txt; do
  count=$((count + 1))
done
test "$count" = 2
