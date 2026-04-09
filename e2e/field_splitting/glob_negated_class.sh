#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Negated character class [!0-9] matches non-digits
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
echo x > abc.txt
echo x > 123.txt
count=0
for f in [!0-9]*.txt; do
  count=$((count + 1))
done
test "$count" = 1
