#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash is literal when followed by non-special char inside double-quotes
# EXPECT_OUTPUT: \a
# EXPECT_EXIT: 0
echo "\a"
