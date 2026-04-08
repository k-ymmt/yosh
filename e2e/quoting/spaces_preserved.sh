#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Double quotes preserve spaces within a field
# EXPECT_OUTPUT: hello   world
echo "hello   world"
