#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Use case - report word counts per file collected via wc
# EXPECT_OUTPUT<<END
# a.txt: 3 words
# b.txt: 5 words
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
echo "one two three" > a.txt
echo "one two three four five" > b.txt
for f in *.txt; do
  # Arithmetic expansion strips wc's leading whitespace padding
  words=$(( $(wc -w < "$f") ))
  echo "$f: $words words"
done
