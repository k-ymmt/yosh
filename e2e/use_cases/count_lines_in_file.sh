#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Use case - count lines in a file with wc and command substitution
# EXPECT_OUTPUT: 4 lines
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
printf '%s\n' alpha beta gamma delta > data.txt
lines=$(wc -l < data.txt)
# Unquoted expansion field-splits away wc's leading whitespace padding
echo $lines lines
