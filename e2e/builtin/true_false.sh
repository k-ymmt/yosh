#!/bin/sh
# POSIX_REF: 4 Utilities - true, false
# DESCRIPTION: true returns 0, false returns 1
# EXPECT_OUTPUT<<END
# 0
# 1
# END
true
echo "$?"
false
echo "$?"
