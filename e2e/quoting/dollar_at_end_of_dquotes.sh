#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Trailing $ in double quotes is literal
# EXPECT_OUTPUT: hello$
echo "hello$"
