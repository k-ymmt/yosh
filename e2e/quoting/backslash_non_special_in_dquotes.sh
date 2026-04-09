#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Backslash before non-special char in double quotes is preserved
# XFAIL: kish double-quote backslash handling duplicates the following character
# EXPECT_OUTPUT<<END
# \a
# \n
# END
echo "\a"
echo "\n"
