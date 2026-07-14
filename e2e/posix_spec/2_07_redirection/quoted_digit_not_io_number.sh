#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: A quoted digit is a word, not an IO_NUMBER
# EXPECT_OUTPUT: x 2
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
echo x "2">f
cat f
