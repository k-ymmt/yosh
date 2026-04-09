#!/bin/sh
# POSIX_REF: 2.13.3 Patterns Used for Filename Expansion
# DESCRIPTION: Glob * does not match dot files
# EXPECT_EXIT: 0
dir="$TEST_TMPDIR/globtest"
mkdir "$dir"
cd "$dir"
echo x > visible.txt
echo x > .hidden.txt
count=0
for f in *; do
  count=$((count + 1))
done
test "$count" = 1
