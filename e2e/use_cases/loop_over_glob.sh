#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Use case - iterate over files matching a glob pattern
# EXPECT_OUTPUT<<END
# processing a.txt
# processing b.txt
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
: > a.txt
: > b.txt
: > c.log
for f in *.txt; do
  echo "processing $f"
done
