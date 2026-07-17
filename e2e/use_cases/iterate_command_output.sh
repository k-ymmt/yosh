#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Use case - iterate over whitespace-separated words of command output
# EXPECT_OUTPUT<<END
# word: one
# word: two
# word: three
# END
for word in $(printf '%s\n' "one two" three); do
  echo "word: $word"
done
