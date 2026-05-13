#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Backslash is literal inside single-quotes (no escape interpretation)
# EXPECT_OUTPUT: \\
# EXPECT_EXIT: 0
echo '\\'
