#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Use case - detect an empty glob match and fall back gracefully
# EXPECT_OUTPUT: no files to process
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
for f in *.dat; do
  # With no match the pattern stays literal; guard with -e before using it
  if [ -e "$f" ]; then
    echo "processing $f"
  else
    echo "no files to process"
  fi
done
