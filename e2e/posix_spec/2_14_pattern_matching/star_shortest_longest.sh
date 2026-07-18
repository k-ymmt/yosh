#!/bin/sh
# POSIX_REF: 2.14 Pattern Matching Notation
# DESCRIPTION: * drives shortest vs longest match in ${v#*X} and ${v##*X}
# EXPECT_OUTPUT<<END
# defXghi
# ghi
# END
# EXPECT_EXIT: 0
v=abcXdefXghi
echo "${v#*X}"
echo "${v##*X}"
