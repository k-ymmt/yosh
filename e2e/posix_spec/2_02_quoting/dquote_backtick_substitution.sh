#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backtick retains its special meaning inside double-quotes
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo "`echo hi`"
