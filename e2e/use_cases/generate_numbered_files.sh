#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Use case - generate zero-padded numbered files with printf
# EXPECT_OUTPUT<<END
# file01.txt
# file02.txt
# file03.txt
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
i=1
while [ "$i" -le 3 ]; do
  name=$(printf 'file%02d.txt' "$i")
  : > "$name"
  i=$((i + 1))
done
for f in *; do
  echo "$f"
done
