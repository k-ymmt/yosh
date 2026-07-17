#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: Use case - branch on whether a path is a directory, file, or absent
# EXPECT_OUTPUT<<END
# subdir: directory
# file.txt: file
# missing: absent
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
mkdir subdir
echo hi > file.txt
for p in subdir file.txt missing; do
  if [ -d "$p" ]; then
    echo "$p: directory"
  elif [ -f "$p" ]; then
    echo "$p: file"
  else
    echo "$p: absent"
  fi
done
