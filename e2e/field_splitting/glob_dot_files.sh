#!/bin/sh
# POSIX_REF: 2.13.3 Patterns Used for Filename Expansion
# DESCRIPTION: Glob * does not match dot files
# XFAIL: kish expansion includes all files regardless of dot prefix
cd "$TEST_TMPDIR"
echo x > visible.txt
echo x > .hidden.txt
count=0
for f in *; do
  count=$((count + 1))
done
test "$count" = 1
