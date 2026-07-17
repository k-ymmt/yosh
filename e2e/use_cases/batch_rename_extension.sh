#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Use case - batch rename files from .txt to .md with suffix removal
# EXPECT_OUTPUT<<END
# notes.md
# readme.md
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
: > notes.txt
: > readme.txt
for f in *.txt; do
  mv "$f" "${f%.txt}.md"
done
for f in *; do
  echo "$f"
done
