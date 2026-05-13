#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word inside double-quotes is a literal word, not a keyword
# EXPECT_OUTPUT: if
# EXPECT_EXIT: 0
echo "if"
